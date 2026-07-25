//! 予約済み YubiKey storage の clear lifecycle を保持する。

use crate::{
    Result,
    features::yubikey_lifecycle::domain::commands::ClearCommand,
    features::yubikey_lifecycle::domain::{
        piv::SecretStorageSpec,
        storage::{
            SecretStorageClearIntent, SecretStorageSetupProbe, SecretStorageStatus,
            is_observed_storage_invalid,
        },
    },
    features::yubikey_lifecycle::ports::public::piv_pin_input::PivPinInputPort,
    features::yubikey_lifecycle::ports::{DeviceSerialPort, SecretStoragePort},
};

/// `--yes` を検証してから PIN を読み、同一 PIV session で観測済み不整合だけを clear する。
///
/// 正常な manifest（保存済み token の有無を問わない）と完全に空の予約領域は clear の許可根拠
/// ではない。status inspection 自体の error も storage 不整合へ読み替えず、破壊操作なしで
/// 伝播する。
pub(crate) fn run_clear(
    command: ClearCommand,
    device: &mut dyn DeviceSerialPort,
    piv_pin: &dyn PivPinInputPort,
    storage: &mut dyn SecretStoragePort,
) -> Result<()> {
    command.ensure_confirmed()?;
    let serial = device.resolve_device_serial(command.serial)?;
    device
        .inspect_device_profile(serial)?
        .ensure_pin_free_recovery_supported()?;
    let pin = piv_pin.read_piv_pin_secret()?;
    storage.begin_piv_management_session(serial, pin)?;

    let inspections = SecretStorageSpec::all_for_serial(serial)
        .into_iter()
        .map(|storage_spec| {
            storage
                .inspect_secret_storage_status(serial, &storage_spec)
                .map(|inspection| (storage_spec, inspection))
        })
        .collect::<Result<Vec<_>>>()?;
    match SecretStorageStatus::from_inspections(inspections) {
        Err(error) if is_observed_storage_invalid(&error) => {
            let inspection = storage
                .inspect_secret_storage_setup(serial, &SecretStorageSetupProbe::expected())?;
            SecretStorageClearIntent::reject_unknown_ownership(&inspection)?;
        }
        Err(error) => return Err(error),
        Ok(status) => {
            if status.stored().is_empty() {
                let inspection = storage
                    .inspect_secret_storage_setup(serial, &SecretStorageSetupProbe::expected())?;
                SecretStorageClearIntent::reject_unknown_ownership(&inspection)?;
            }
            anyhow::bail!(
                "refusing to clear YubiKey secret storage unless reserved storage is observably invalid"
            )
        }
    }

    let intent = SecretStorageClearIntent::expected();
    let public_key_spki = storage.clear_secret_storage(serial, intent.clone())?;
    storage.finalize_secret_storage_setup(
        serial,
        intent.manifest_for_generated_public_key(public_key_spki)?,
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        features::yubikey_lifecycle::{
            domain::{
                self as domain,
                commands::ClearCommand,
                manifest::SecretManifest,
                piv::{PivApplicationVersion, SecretName},
                storage::{SecretStorageSetupInspection, SecretStorageStatusInspection},
            },
            ports::public::{MockDeviceSerialPort, MockSecretStoragePort},
        },
        foundation::protection::ProtectedSecret,
    };

    use super::run_clear;

    fn expect_pin_free_device_profile(device: &mut MockDeviceSerialPort) {
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

    #[test]
    fn clear_without_yes_does_not_read_pin_or_start_session() {
        let mut device = MockDeviceSerialPort::new();
        device.expect_resolve_device_serial().times(0);
        let mut pin = crate::features::yubikey_lifecycle::ports::public::piv_pin_input::MockPivPinInputPort::new();
        pin.expect_read_piv_pin_secret().times(0);
        let mut storage = MockSecretStoragePort::new();
        storage.expect_begin_piv_management_session().times(0);
        storage.expect_clear_secret_storage().times(0);

        assert!(
            run_clear(
                ClearCommand {
                    serial: None,
                    confirmed: false,
                },
                &mut device,
                &pin,
                &mut storage,
            )
            .is_err()
        );
    }

    fn stored_status() -> crate::Result<SecretStorageStatusInspection> {
        Ok(SecretStorageStatusInspection {
            manifest_present: true,
            manifest_bytes: Some(SecretManifest::fixture_v2().encode()?),
            object_present: true,
            object_exists: true,
        })
    }

    fn empty_status() -> SecretStorageStatusInspection {
        SecretStorageStatusInspection {
            manifest_present: false,
            manifest_bytes: None,
            object_present: false,
            object_exists: false,
        }
    }

    fn configure_confirmed_session(
        device: &mut MockDeviceSerialPort,
        pin: &mut crate::features::yubikey_lifecycle::ports::public::piv_pin_input::MockPivPinInputPort,
        storage: &mut MockSecretStoragePort,
    ) {
        expect_pin_free_device_profile(device);
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Ok(2001));
        pin.expect_read_piv_pin_secret()
            .times(1)
            .returning(|| ProtectedSecret::from_test_bytes(b"123456"));
        storage
            .expect_begin_piv_management_session()
            .times(1)
            .withf(|serial, _| *serial == 2001)
            .returning(|_, _| Ok(()));
    }

    #[test]
    fn clear_yes_refuses_normal_existing_storage_without_mutation() {
        let mut device = MockDeviceSerialPort::new();
        let mut pin = crate::features::yubikey_lifecycle::ports::public::piv_pin_input::MockPivPinInputPort::new();
        let mut storage = MockSecretStoragePort::new();
        configure_confirmed_session(&mut device, &mut pin, &mut storage);
        storage
            .expect_inspect_secret_storage_status()
            .times(1)
            .withf(|serial, spec| *serial == 2001 && spec.name == SecretName::BitwardenClientSecret)
            .returning(|_, _| stored_status());
        storage.expect_clear_secret_storage().never();
        storage.expect_finalize_secret_storage_setup().never();

        assert!(
            run_clear(
                ClearCommand {
                    serial: Some(2001),
                    confirmed: true,
                },
                &mut device,
                &pin,
                &mut storage,
            )
            .is_err()
        );
    }

    #[test]
    fn clear_yes_refuses_empty_storage_without_mutation() {
        let mut device = MockDeviceSerialPort::new();
        let mut pin = crate::features::yubikey_lifecycle::ports::public::piv_pin_input::MockPivPinInputPort::new();
        let mut storage = MockSecretStoragePort::new();
        configure_confirmed_session(&mut device, &mut pin, &mut storage);
        storage
            .expect_inspect_secret_storage_status()
            .times(1)
            .returning(|_, _| Ok(empty_status()));
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .returning(|_, _| {
                Ok(SecretStorageSetupInspection {
                    reserved_slot_key_exists: false,
                    reserved_slot_certificate_exists: false,
                    slot_public_key_spki: None,
                    piv_version: PivApplicationVersion::minimum_for_secret_storage(),
                    manifest_bytes: None,
                    present_object_ids: Vec::new(),
                    nonempty_object_ids: Vec::new(),
                })
            });
        storage.expect_clear_secret_storage().never();
        storage.expect_finalize_secret_storage_setup().never();

        assert!(
            run_clear(
                ClearCommand {
                    serial: Some(2001),
                    confirmed: true,
                },
                &mut device,
                &pin,
                &mut storage,
            )
            .is_err()
        );
    }

    #[test]
    fn clear_yes_escalates_key_only_ownership_without_mutation() -> crate::Result<()> {
        let mut device = MockDeviceSerialPort::new();
        let mut pin = crate::features::yubikey_lifecycle::ports::public::piv_pin_input::MockPivPinInputPort::new();
        let mut storage = MockSecretStoragePort::new();
        configure_confirmed_session(&mut device, &mut pin, &mut storage);
        storage
            .expect_inspect_secret_storage_status()
            .times(1)
            .returning(|_, _| Ok(empty_status()));
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
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
        storage.expect_clear_secret_storage().never();
        storage.expect_finalize_secret_storage_setup().never();

        let error = run_clear(
            ClearCommand {
                serial: Some(2001),
                confirmed: true,
            },
            &mut device,
            &pin,
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
    fn clear_yes_clears_only_observed_storage_invalid() -> crate::Result<()> {
        let mut device = MockDeviceSerialPort::new();
        let mut pin = crate::features::yubikey_lifecycle::ports::public::piv_pin_input::MockPivPinInputPort::new();
        let mut storage = MockSecretStoragePort::new();
        configure_confirmed_session(&mut device, &mut pin, &mut storage);
        storage
            .expect_inspect_secret_storage_status()
            .times(1)
            .returning(|_, _| {
                Ok(SecretStorageStatusInspection {
                    manifest_present: true,
                    manifest_bytes: None,
                    object_present: true,
                    object_exists: false,
                })
            });
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .returning(|_, _| {
                Ok(SecretStorageSetupInspection {
                    reserved_slot_key_exists: false,
                    reserved_slot_certificate_exists: false,
                    slot_public_key_spki: None,
                    piv_version: PivApplicationVersion::minimum_for_secret_storage(),
                    manifest_bytes: None,
                    present_object_ids: vec![SecretName::BitwardenClientSecret.object_id()],
                    nonempty_object_ids: Vec::new(),
                })
            });
        storage
            .expect_clear_secret_storage()
            .times(1)
            .withf(|serial, _| *serial == 2001)
            .returning(|_, _| {
                SecretManifest::fixture_v2()
                    .slot_public_key_spki
                    .ok_or_else(|| anyhow::anyhow!("fixture v2 manifest must contain SPKI"))
            });
        storage
            .expect_finalize_secret_storage_setup()
            .times(1)
            .withf(|serial, manifest| {
                *serial == 2001
                    && SecretManifest::decode(manifest)
                        .is_ok_and(|manifest| manifest == SecretManifest::fixture_v2())
            })
            .returning(|_, _| Ok(()));

        run_clear(
            ClearCommand {
                serial: Some(2001),
                confirmed: true,
            },
            &mut device,
            &pin,
            &mut storage,
        )
    }

    #[test]
    fn clear_yes_propagates_status_transport_error_without_mutation() {
        let mut device = MockDeviceSerialPort::new();
        let mut pin = crate::features::yubikey_lifecycle::ports::public::piv_pin_input::MockPivPinInputPort::new();
        let mut storage = MockSecretStoragePort::new();
        configure_confirmed_session(&mut device, &mut pin, &mut storage);
        storage
            .expect_inspect_secret_storage_status()
            .times(1)
            .returning(|_, _| Err(anyhow::anyhow!("PC/SC transport failed")));
        storage.expect_clear_secret_storage().never();
        storage.expect_finalize_secret_storage_setup().never();

        let error = run_clear(
            ClearCommand {
                serial: Some(2001),
                confirmed: true,
            },
            &mut device,
            &pin,
            &mut storage,
        )
        .expect_err("transport error must stop before mutation");
        assert!(error.to_string().contains("PC/SC transport failed"));
    }
}
