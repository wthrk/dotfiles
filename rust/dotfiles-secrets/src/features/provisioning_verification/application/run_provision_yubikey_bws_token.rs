//! provisioning source の BWS token 保存を、単一 PIV 管理 session に閉じる。
//!
//! shell の process 境界をまたぐ `status → clear/setup → put` は使わない。PIV PIN を一回だけ
//! 取得して高水準 session を開始した後、観測から local decrypt 検証まで同じ storage port に適用する。

use crate::{
    Result,
    features::provisioning_verification::domain::commands::ProvisionBwsTokenCommand,
    features::{
        cli_interaction::ports::public::BitwardenClientSecretInputPort,
        yubikey_lifecycle::{
            domain::storage::{
                SecretStorageReadIntent, SecretStorageSetupIntent, SecretStorageSetupProbe,
                SecretStorageStatus, SecretStorageVerificationPlan, SecretStorageWriteIntent,
            },
            ports::public::piv_pin_input::PivPinInputPort,
            ports::{DeviceSerialPort, SecretStoragePort},
        },
    },
};

/// BWS access token を必要時だけ保存し、同一 PIV handle で復号可能なことまで確認する。
///
/// serial を確定してから PIN を一回だけ読み、`begin_piv_management_session` に同じ serial と PIN を
/// 渡す。正常な既存 token は再入力・書込みをせず local decrypt 検証だけを行う。
/// source provisioning は `enroll-primary` / `enroll-spare` が確定した InitializedV2 だけを使い、
/// fresh、version 1、manifestless/zero-length partial、ownership 不明を入力前・mutation 前に停止する。
/// opt-in debug の presentation はこの use case 外の composition decorator が通常の port 呼び出しを
/// 観測して担い、use case は表示用 DTO や technical failure 分類を持たない。
pub(crate) fn run_provision_yubikey_bws_token(
    command: ProvisionBwsTokenCommand,
    device: &mut dyn DeviceSerialPort,
    piv_pin: &dyn PivPinInputPort,
    secret_input: &dyn BitwardenClientSecretInputPort,
    storage: &mut dyn SecretStoragePort,
) -> Result<()> {
    let serial = device.resolve_device_serial(command.serial)?;
    device
        .inspect_device_profile(serial)?
        .ensure_pin_free_recovery_supported()?;
    let pin = piv_pin.read_piv_pin_secret()?;
    storage.begin_piv_management_session(serial, pin)?;

    let storage_spec = command.storage_spec(serial);
    let setup_inspection =
        storage.inspect_secret_storage_setup(serial, &SecretStorageSetupProbe::expected())?;
    let _initialized = SecretStorageSetupIntent::for_initialized_provisioning(setup_inspection)?;

    let status_inspection = storage.inspect_secret_storage_status(serial, &storage_spec)?;
    let status =
        SecretStorageStatus::from_inspections([(storage_spec.clone(), status_inspection)])?;
    if status.stored().contains(&storage_spec.name) {
        return verify_local_storage(serial, storage);
    }

    let inspection = storage.inspect_secret_storage_write(serial, &storage_spec)?;
    let preflight =
        SecretStorageWriteIntent::preflight_initial_enrollment(storage_spec, &inspection)?;
    let token = secret_input.read_bitwarden_client_secret()?;
    let intent = preflight.with_initial_enrollment_secret_len(token.len())?;
    storage.store_secret(serial, intent, &token)?;
    verify_local_storage(serial, storage)
}

/// 保存済み token が同一 PIV session で復号・検証できることを確認する。
///
/// inspection、domain intent、load、value validation のいずれかで失敗した時点で caller へ返し、
/// token の再入力や storage mutation を行わない。
fn verify_local_storage(serial: u32, storage: &mut dyn SecretStoragePort) -> Result<()> {
    for storage_spec in SecretStorageVerificationPlan::for_serial(serial).into_targets() {
        let inspection = storage.inspect_secret_storage_read(serial, &storage_spec)?;
        let intent = SecretStorageReadIntent::from_inspection(storage_spec, inspection)?;
        let secret = storage
            .load_secret(serial, &intent)
            .map_err(|error| intent.decode_error(error))?;
        intent.validate_loaded_secret(&secret)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! `provision-bws-token` の fail-closed lifecycle を port mock で確認する。

    use crate::{
        features::{
            cli_interaction::ports::io::MockBitwardenClientSecretInputPort,
            provisioning_verification::domain::commands::ProvisionBwsTokenCommand,
            yubikey_lifecycle::domain::{
                self as domain,
                manifest::SecretManifest,
                piv::{PivApplicationVersion, SecretName},
                storage::{
                    SecretStorageReadInspection, SecretStorageSetupInspection,
                    SecretStorageStatusInspection, SecretStorageWriteInspection,
                },
            },
        },
        foundation::protection::ProtectedSecret,
    };

    use super::run_provision_yubikey_bws_token;

    fn expect_pin_free_device_profile(
        device: &mut crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort,
    ) {
        device.expect_inspect_device_profile().returning(|_| {
            Ok(domain::piv::PivDeviceProfile {
                version: domain::piv::PivApplicationVersion {
                    major: 5,
                    minor: 7,
                    patch: 1,
                },
                fips_series: false,
            })
        });
    }

    fn initialized_setup() -> crate::Result<SecretStorageSetupInspection> {
        Ok(SecretStorageSetupInspection {
            reserved_slot_key_exists: true,
            reserved_slot_certificate_exists: false,
            slot_public_key_spki: SecretManifest::fixture_v2().slot_public_key_spki,
            piv_version: PivApplicationVersion::minimum_for_secret_storage(),
            manifest_bytes: Some(SecretManifest::fixture_v2().encode()?),
            present_object_ids: vec![domain::piv::PivObjectId::MANIFEST],
            nonempty_object_ids: vec![domain::piv::PivObjectId::MANIFEST],
        })
    }

    fn initialized_write() -> crate::Result<SecretStorageWriteInspection> {
        Ok(SecretStorageWriteInspection {
            manifest_present: true,
            manifest_bytes: Some(SecretManifest::fixture_v2().encode()?),
            object_present: false,
            object_exists: false,
            reserved_slot_key_exists: true,
            reserved_slot_certificate_exists: false,
            slot_public_key_spki: SecretManifest::fixture_v2().slot_public_key_spki,
        })
    }

    #[test]
    fn existing_token_is_verified_without_token_input_or_mutation() -> crate::Result<()> {
        let mut device =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        expect_pin_free_device_profile(&mut device);
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut pin = crate::features::yubikey_lifecycle::ports::public::piv_pin_input::MockPivPinInputPort::new();
        pin.expect_read_piv_pin_secret()
            .returning(|| ProtectedSecret::from_test_bytes(b"123456"));
        let mut input = MockBitwardenClientSecretInputPort::new();
        input.expect_read_bitwarden_client_secret().never();
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_management_session()
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_setup()
            .returning(|_, _| initialized_setup());
        storage
            .expect_inspect_secret_storage_status()
            .returning(|_, _| {
                Ok(SecretStorageStatusInspection {
                    manifest_present: true,
                    manifest_bytes: Some(SecretManifest::fixture_v2().encode()?),
                    object_present: true,
                    object_exists: true,
                })
            });
        storage
            .expect_inspect_secret_storage_read()
            .returning(|_, _| {
                Ok(SecretStorageReadInspection {
                    manifest_bytes: Some(SecretManifest::fixture_v2().encode()?),
                    encoded: Some(vec![1]),
                })
            });
        storage
            .expect_load_secret()
            .returning(|_, _| ProtectedSecret::from_test_bytes(b"token"));
        storage.expect_store_secret().never();
        storage.expect_clear_secret_storage().never();

        run_provision_yubikey_bws_token(
            ProvisionBwsTokenCommand { serial: Some(2001) },
            &mut device,
            &pin,
            &input,
            &mut storage,
        )?;
        Ok(())
    }

    #[test]
    fn key_only_partial_state_escalates_without_token_input_or_mutation() -> crate::Result<()> {
        let mut device =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        expect_pin_free_device_profile(&mut device);
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut pin = crate::features::yubikey_lifecycle::ports::public::piv_pin_input::MockPivPinInputPort::new();
        pin.expect_read_piv_pin_secret()
            .returning(|| ProtectedSecret::from_test_bytes(b"123456"));
        let mut input = MockBitwardenClientSecretInputPort::new();
        input.expect_read_bitwarden_client_secret().never();
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_management_session()
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_setup()
            .returning(|_, _| {
                Ok(SecretStorageSetupInspection {
                    reserved_slot_key_exists: true,
                    reserved_slot_certificate_exists: false,
                    slot_public_key_spki: SecretManifest::fixture_v2().slot_public_key_spki,
                    piv_version: PivApplicationVersion::minimum_for_secret_storage(),
                    manifest_bytes: None,
                    present_object_ids: Vec::new(),
                    nonempty_object_ids: Vec::new(),
                })
            });
        storage.expect_inspect_secret_storage_status().never();
        storage.expect_initialize_secret_storage().never();
        storage.expect_finalize_secret_storage_setup().never();
        storage.expect_clear_secret_storage().never();
        storage.expect_store_secret().never();

        let error = run_provision_yubikey_bws_token(
            ProvisionBwsTokenCommand { serial: Some(2001) },
            &mut device,
            &pin,
            &input,
            &mut storage,
        )
        .expect_err("key-only ownership must require manual escalation");
        assert!(
            error
                .to_string()
                .contains("manual administrator escalation")
        );
        Ok(())
    }

    #[test]
    fn every_non_initialized_v2_partial_state_stops_before_token_input_or_mutation()
    -> crate::Result<()> {
        let v1 = SecretManifest {
            version: 1,
            app: domain::manifest::MANIFEST_APP.to_owned(),
            slot_public_key_spki: None,
        }
        .encode()?;
        let fixture_spki = SecretManifest::fixture_v2()
            .slot_public_key_spki
            .ok_or_else(|| anyhow::anyhow!("fixture v2 manifest must contain SPKI"))?;
        let mut mismatched_spki = fixture_spki.clone();
        let last = mismatched_spki
            .last_mut()
            .ok_or_else(|| anyhow::anyhow!("fixture SPKI must not be empty"))?;
        *last ^= 1;
        for inspection in [
            SecretStorageSetupInspection {
                reserved_slot_key_exists: true,
                reserved_slot_certificate_exists: false,
                slot_public_key_spki: Some(fixture_spki.clone()),
                piv_version: PivApplicationVersion::minimum_for_secret_storage(),
                manifest_bytes: Some(v1),
                present_object_ids: vec![domain::piv::PivObjectId::MANIFEST],
                nonempty_object_ids: vec![domain::piv::PivObjectId::MANIFEST],
            },
            SecretStorageSetupInspection {
                reserved_slot_key_exists: false,
                reserved_slot_certificate_exists: false,
                slot_public_key_spki: None,
                piv_version: PivApplicationVersion::minimum_for_secret_storage(),
                manifest_bytes: None,
                present_object_ids: vec![SecretName::BitwardenClientSecret.object_id()],
                nonempty_object_ids: vec![SecretName::BitwardenClientSecret.object_id()],
            },
            SecretStorageSetupInspection {
                reserved_slot_key_exists: true,
                reserved_slot_certificate_exists: false,
                slot_public_key_spki: Some(mismatched_spki),
                piv_version: PivApplicationVersion::minimum_for_secret_storage(),
                manifest_bytes: Some(SecretManifest::fixture_v2().encode()?),
                present_object_ids: vec![domain::piv::PivObjectId::MANIFEST],
                nonempty_object_ids: vec![domain::piv::PivObjectId::MANIFEST],
            },
            SecretStorageSetupInspection {
                reserved_slot_key_exists: true,
                reserved_slot_certificate_exists: false,
                slot_public_key_spki: Some(fixture_spki.clone()),
                piv_version: PivApplicationVersion::minimum_for_secret_storage(),
                manifest_bytes: None,
                present_object_ids: vec![domain::piv::PivObjectId::MANIFEST],
                nonempty_object_ids: Vec::new(),
            },
        ] {
            let mut device =
                crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
            expect_pin_free_device_profile(&mut device);
            device
                .expect_resolve_device_serial()
                .returning(|_| Ok(2001));
            let mut pin = crate::features::yubikey_lifecycle::ports::public::piv_pin_input::MockPivPinInputPort::new();
            pin.expect_read_piv_pin_secret()
                .returning(|| ProtectedSecret::from_test_bytes(b"123456"));
            let mut input = MockBitwardenClientSecretInputPort::new();
            input.expect_read_bitwarden_client_secret().never();
            let mut storage =
                crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
            storage
                .expect_begin_piv_management_session()
                .returning(|_, _| Ok(()));
            let mut inspection = Some(inspection);
            storage
                .expect_inspect_secret_storage_setup()
                .returning(move |_, _| {
                    inspection
                        .take()
                        .ok_or_else(|| anyhow::anyhow!("single inspection was already consumed"))
                });
            storage.expect_inspect_secret_storage_status().never();
            storage.expect_initialize_secret_storage().never();
            storage.expect_finalize_secret_storage_setup().never();
            storage.expect_clear_secret_storage().never();
            storage.expect_store_secret().never();

            assert!(
                run_provision_yubikey_bws_token(
                    ProvisionBwsTokenCommand { serial: Some(2001) },
                    &mut device,
                    &pin,
                    &input,
                    &mut storage,
                )
                .is_err()
            );
        }
        Ok(())
    }

    #[test]
    fn initialized_empty_storage_reads_token_once_then_stores_and_verifies() -> crate::Result<()> {
        let mut device =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        expect_pin_free_device_profile(&mut device);
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut pin = crate::features::yubikey_lifecycle::ports::public::piv_pin_input::MockPivPinInputPort::new();
        pin.expect_read_piv_pin_secret()
            .returning(|| ProtectedSecret::from_test_bytes(b"123456"));
        let mut input = MockBitwardenClientSecretInputPort::new();
        input
            .expect_read_bitwarden_client_secret()
            .times(1)
            .returning(|| ProtectedSecret::from_test_bytes(b"token"));
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_management_session()
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_setup()
            .returning(|_, _| initialized_setup());
        storage
            .expect_inspect_secret_storage_status()
            .returning(|_, _| {
                Ok(SecretStorageStatusInspection {
                    manifest_present: true,
                    manifest_bytes: Some(SecretManifest::fixture_v2().encode()?),
                    object_present: false,
                    object_exists: false,
                })
            });
        storage
            .expect_inspect_secret_storage_write()
            .returning(|_, _| initialized_write());
        storage
            .expect_store_secret()
            .times(1)
            .returning(|_, _, _| Ok(()));
        storage
            .expect_inspect_secret_storage_read()
            .returning(|_, _| {
                Ok(SecretStorageReadInspection {
                    manifest_bytes: Some(SecretManifest::fixture_v2().encode()?),
                    encoded: Some(vec![1]),
                })
            });
        storage
            .expect_load_secret()
            .returning(|_, _| ProtectedSecret::from_test_bytes(b"token"));

        run_provision_yubikey_bws_token(
            ProvisionBwsTokenCommand { serial: Some(2001) },
            &mut device,
            &pin,
            &input,
            &mut storage,
        )
    }
}
