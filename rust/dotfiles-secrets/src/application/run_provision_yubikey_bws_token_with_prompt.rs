//! provisioning source の BWS token 保存を、単一 PIV 管理 session に閉じる。
//!
//! Yubico PIV の PIN-protected management key は、同一 session で PIN VERIFY 後にだけ使える。
//! この use case は shell の process 境界をまたぐ `status → clear/setup → put` を禁止し、観測から
//! local decrypt 検証までを一つの storage port session に適用する。PIN policy の背景は
//! <https://docs.yubico.com/yesdk/users-manual/application-piv/pin-only.html#pin-protected>、
//! management 操作の policy は
//! <https://docs.yubico.com/yesdk/users-manual/application-piv/pin-touch-policies.html> を正本とする。

use crate::Result;
use crate::{
    domain::{
        commands::ProvisionBwsTokenCommand,
        storage::{
            SecretStorageClearIntent, SecretStorageReadIntent, SecretStorageSetupIntent,
            SecretStorageSetupProbe, SecretStorageStatus, SecretStorageStatusInvalid,
            SecretStorageVerificationPlan, SecretStorageWriteIntent,
        },
    },
    ports,
};

/// `provision-bws-token` が必要とする外部 capability を束ねる。
pub(crate) struct ProvisionBwsTokenRuntime<'a> {
    pub(crate) device: &'a mut dyn ports::DeviceSerialPort,
    pub(crate) piv_pin: &'a dyn ports::PivPinInputPort,
    pub(crate) secret_input: &'a dyn ports::SecretInputPort,
    pub(crate) storage: &'a mut dyn ports::SecretStoragePort,
}

/// BWS access token を必要時だけ保存し、同一 PIV handle で復号可能なことまで確認する。
///
/// serial を先に確定してから PIN を一度だけ hidden TTY で読み、`begin_piv_management_session`
/// 以降は同じ serial の storage port を使う。正常に token が保存済みなら書込みも token 入力もせず
/// 終了する。予約領域の不整合だけは domain の typed error を根拠に clear して空の v2 manifest を
/// 再確定し、完全に空の領域だけは setup する。transport / discovery / SDK error を storage 不整合へ
/// 推測して clear する経路はない。
pub(crate) fn run_provision_yubikey_bws_token_with_prompt(
    command: ProvisionBwsTokenCommand,
    runtime: ProvisionBwsTokenRuntime<'_>,
) -> Result<()> {
    let ProvisionBwsTokenRuntime {
        device,
        piv_pin,
        secret_input,
        storage: storage_port,
    } = runtime;
    let serial = device.resolve_device_serial(command.serial)?;

    // Status/read/write/clear/setup を同一 PIV handle に載せるため、PIN は status より前に
    // 一度だけ要求する。token 本文は preflight が成功するまで取得しない。
    storage_port.begin_piv_management_session(piv_pin.read_piv_pin_secret()?)?;
    let storage = command.storage_spec(serial);
    let status = SecretStorageStatus::from_inspections([(
        storage.clone(),
        storage_port.inspect_secret_storage_status(serial, &storage)?,
    )]);

    match status {
        Ok(status) if status.stored().contains(&storage.name) => return Ok(()),
        Ok(_) => ensure_initialized_storage(serial, storage_port)?,
        Err(error) if is_observed_storage_invalid(&error) => {
            let clear = SecretStorageClearIntent::expected();
            let public_key_spki = storage_port.clear_secret_storage(serial, clear.clone())?;
            storage_port.finalize_secret_storage_setup(
                serial,
                clear.manifest_for_generated_public_key(public_key_spki)?,
            )?;
        }
        Err(error) => return Err(error),
    }

    let inspection = storage_port.inspect_secret_storage_write(serial, &storage)?;
    // This is before token input: a malformed post-clear/setup state cannot consume the token.
    let _preflight = SecretStorageWriteIntent::preflight_put(storage.clone(), &inspection, false)?;
    let token = secret_input.read_bitwarden_client_secret_tty_secret()?;
    let intent = SecretStorageWriteIntent::put(storage, inspection, false, token.len())?;
    storage_port.store_secret(serial, intent, &token)?;
    verify_local_storage(serial, storage_port)
}

fn is_observed_storage_invalid(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.is::<SecretStorageStatusInvalid>())
}

fn ensure_initialized_storage(
    serial: u32,
    storage_port: &mut dyn ports::SecretStoragePort,
) -> Result<()> {
    let probe = SecretStorageSetupProbe::expected();
    let inspection = storage_port.inspect_secret_storage_setup(serial, &probe)?;
    let intent = SecretStorageSetupIntent::from_inspection(inspection)?;
    if !intent.requires_public_key_spki() {
        return Ok(());
    }
    let public_key_spki = storage_port.initialize_secret_storage(serial, intent.clone())?;
    if intent.requires_finalization() {
        storage_port.finalize_secret_storage_setup(
            serial,
            intent.manifest_for_public_key(public_key_spki)?,
        )?;
    }
    Ok(())
}

fn verify_local_storage(
    serial: u32,
    storage_port: &mut dyn ports::SecretStoragePort,
) -> Result<()> {
    for storage in SecretStorageVerificationPlan::for_serial(serial).into_targets() {
        let inspection = storage_port.inspect_secret_storage_read(serial, &storage)?;
        let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
        let secret = storage_port
            .load_secret(serial, &intent)
            .map_err(|error| intent.decode_error(error))?;
        intent.validate_loaded_secret(&secret)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{
            commands::ProvisionBwsTokenCommand,
            manifest::SecretManifest,
            piv::{PivApplicationVersion, SecretName},
            storage::{
                SecretStorageReadInspection, SecretStorageSetupInspection,
                SecretStorageStatusInspection, SecretStorageWriteInspection,
            },
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::{ProvisionBwsTokenRuntime, run_provision_yubikey_bws_token_with_prompt};

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn manifest() -> Vec<u8> {
        SecretManifest::fixture_v2().encode().expect("manifest")
    }

    fn empty_status() -> SecretStorageStatusInspection {
        SecretStorageStatusInspection {
            manifest_bytes: None,
            object_present: false,
            object_exists: false,
        }
    }

    fn stored_status() -> SecretStorageStatusInspection {
        SecretStorageStatusInspection {
            manifest_bytes: Some(manifest()),
            object_present: true,
            object_exists: true,
        }
    }

    fn write_inspection() -> SecretStorageWriteInspection {
        SecretStorageWriteInspection {
            manifest_bytes: Some(manifest()),
            object_present: false,
            object_exists: false,
            reserved_slot_key_exists: true,
            reserved_slot_certificate_exists: false,
            slot_public_key_spki: Some(
                SecretManifest::fixture_v2()
                    .slot_public_key_spki
                    .expect("fixture SPKI"),
            ),
        }
    }

    fn read_inspection() -> SecretStorageReadInspection {
        SecretStorageReadInspection {
            manifest_bytes: Some(manifest()),
            encoded: Some(vec![1]),
        }
    }

    fn setup_inspection() -> SecretStorageSetupInspection {
        SecretStorageSetupInspection {
            key_exists: false,
            piv_version: PivApplicationVersion::minimum_for_secret_storage(),
            manifest_bytes: None,
            occupied_object_ids: Vec::new(),
        }
    }

    #[test]
    fn stored_token_uses_one_management_session_but_never_reads_or_mutates_token()
    -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        let mut pin = ports::io::MockPivPinInputPort::new();
        pin.expect_read_piv_pin_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(b"123456")));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_management_session()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(()));
        storage
            .expect_inspect_secret_storage_status()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(stored_status()));
        storage.expect_clear_secret_storage().never();
        storage.expect_inspect_secret_storage_setup().never();
        storage.expect_store_secret().never();
        let mut input = ports::MockSecretInputPort::new();
        input
            .expect_read_bitwarden_client_secret_tty_secret()
            .never();

        run_provision_yubikey_bws_token_with_prompt(
            ProvisionBwsTokenCommand { serial: None },
            ProvisionBwsTokenRuntime {
                device: &mut device,
                piv_pin: &pin,
                secret_input: &input,
                storage: &mut storage,
            },
        )
    }

    #[test]
    fn empty_storage_initializes_stores_and_verifies_in_one_session() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        let mut pin = ports::io::MockPivPinInputPort::new();
        pin.expect_read_piv_pin_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(b"123456")));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_management_session()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(()));
        storage
            .expect_inspect_secret_storage_status()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(empty_status()));
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(setup_inspection()));
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| {
                Ok(SecretManifest::fixture_v2()
                    .slot_public_key_spki
                    .expect("fixture SPKI"))
            });
        storage
            .expect_finalize_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(write_inspection()));
        let mut input = ports::MockSecretInputPort::new();
        input
            .expect_read_bitwarden_client_secret_tty_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(b"access-token")));
        storage
            .expect_store_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|serial, intent, token| {
                *serial == 2001
                    && intent.storage.name == SecretName::BitwardenClientSecret
                    && token.len() == b"access-token".len()
            })
            .returning(|_, _, _| Ok(()));
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(read_inspection()));
        storage
            .expect_load_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(material(b"access-token")));

        run_provision_yubikey_bws_token_with_prompt(
            ProvisionBwsTokenCommand { serial: Some(2001) },
            ProvisionBwsTokenRuntime {
                device: &mut device,
                piv_pin: &pin,
                secret_input: &input,
                storage: &mut storage,
            },
        )
    }

    #[test]
    fn observed_invalid_storage_clears_before_token_input() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        let mut pin = ports::io::MockPivPinInputPort::new();
        pin.expect_read_piv_pin_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(b"123456")));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_management_session()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(()));
        storage
            .expect_inspect_secret_storage_status()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| {
                Ok(SecretStorageStatusInspection {
                    manifest_bytes: None,
                    object_present: true,
                    object_exists: true,
                })
            });
        storage
            .expect_clear_secret_storage()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| {
                Ok(SecretManifest::fixture_v2()
                    .slot_public_key_spki
                    .expect("fixture SPKI"))
            });
        storage
            .expect_finalize_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));
        storage.expect_inspect_secret_storage_setup().never();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(write_inspection()));
        let mut input = ports::MockSecretInputPort::new();
        input
            .expect_read_bitwarden_client_secret_tty_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(b"access-token")));
        storage
            .expect_store_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _, _| Ok(()));
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(read_inspection()));
        storage
            .expect_load_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(material(b"access-token")));

        run_provision_yubikey_bws_token_with_prompt(
            ProvisionBwsTokenCommand { serial: Some(2001) },
            ProvisionBwsTokenRuntime {
                device: &mut device,
                piv_pin: &pin,
                secret_input: &input,
                storage: &mut storage,
            },
        )
    }

    #[test]
    fn unobservable_status_error_never_clears_or_reads_token() {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut pin = ports::io::MockPivPinInputPort::new();
        pin.expect_read_piv_pin_secret()
            .returning(|| Ok(material(b"123456")));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_management_session()
            .returning(|_| Ok(()));
        storage
            .expect_inspect_secret_storage_status()
            .returning(|_, _| Err(anyhow::anyhow!("PC/SC transport failed")));
        storage.expect_clear_secret_storage().never();
        storage.expect_store_secret().never();
        let mut input = ports::MockSecretInputPort::new();
        input
            .expect_read_bitwarden_client_secret_tty_secret()
            .never();

        assert!(
            run_provision_yubikey_bws_token_with_prompt(
                ProvisionBwsTokenCommand { serial: Some(2001) },
                ProvisionBwsTokenRuntime {
                    device: &mut device,
                    piv_pin: &pin,
                    secret_input: &input,
                    storage: &mut storage,
                },
            )
            .is_err()
        );
    }
}
