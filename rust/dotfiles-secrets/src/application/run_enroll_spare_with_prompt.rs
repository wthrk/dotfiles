//! enroll-spare(prompt) の順序を固定し、primary/spare 判定責務を I/O 実装から分離する。

use crate::Result;
use crate::{
    domain::{
        commands::EnrollSpareCommand,
        enrollment::EnrollSummary,
        manifest::BootstrapSecretDocument,
        piv::validate_piv_pin_len,
        storage::{
            SecretStorageReadIntent, SecretStorageSetupIntent, SecretStorageSetupProbe,
            SecretStorageVerificationPlan, SecretStorageWriteIntent,
        },
    },
    ports,
};

/// primary YubiKey から読み出した secret を prompt 運用の spare YubiKey へ複製する。
///
/// primary 読み出し後に spare 解決へ進む順序を固定し、1 reader で primary から spare へ
/// 差し替える運用を保つ。secret 転送手段の詳細は port 境界で読み出しと保存を接続する。
pub(crate) fn run_enroll_spare_with_prompt<D, P, S, R>(
    command: EnrollSpareCommand,
    primary_device: &mut D,
    spare_device: &mut impl ports::SpareDeviceSerialPort,
    pin_policy: &mut impl ports::DevicePinPolicyPort,
    process: &P,
    storage_port: &mut S,
    report: &R,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    P: ports::PinInputPort,
    S: ports::SecretStoragePort,
    R: ports::ReportPort,
{
    command.ensure_requested_serials_distinct()?;
    let primary_serial = primary_device.resolve_device_serial(command.primary_serial)?;
    let primary_pin = if pin_policy.device_requires_pin(primary_serial)? {
        let pin = process.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };
    let [first_storage, second_storage, third_storage] =
        SecretStorageVerificationPlan::for_serial(primary_serial).into_targets();
    let first_inspection =
        storage_port.inspect_secret_storage_read(primary_serial, &first_storage)?;
    let first_intent = SecretStorageReadIntent::from_inspection(first_storage, first_inspection)?;
    let first_document_storage = first_intent.storage.clone();
    let first = storage_port
        .load_secret(primary_serial, &first_intent, primary_pin.as_ref())
        .map_err(|error| first_intent.decode_error(error))?;
    first_intent.validate_loaded_secret(&first)?;
    let second_inspection =
        storage_port.inspect_secret_storage_read(primary_serial, &second_storage)?;
    let second_intent =
        SecretStorageReadIntent::from_inspection(second_storage, second_inspection)?;
    let second_document_storage = second_intent.storage.clone();
    let second = storage_port
        .load_secret(primary_serial, &second_intent, primary_pin.as_ref())
        .map_err(|error| second_intent.decode_error(error))?;
    second_intent.validate_loaded_secret(&second)?;
    let third_inspection =
        storage_port.inspect_secret_storage_read(primary_serial, &third_storage)?;
    let third_intent = SecretStorageReadIntent::from_inspection(third_storage, third_inspection)?;
    let third_document_storage = third_intent.storage.clone();
    let third = storage_port
        .load_secret(primary_serial, &third_intent, primary_pin.as_ref())
        .map_err(|error| third_intent.decode_error(error))?;
    third_intent.validate_loaded_secret(&third)?;
    let document = BootstrapSecretDocument::from_storage_materials([
        (first_document_storage, first),
        (second_document_storage, second),
        (third_document_storage, third),
    ])?;
    let spare_serial = spare_device.resolve_spare_device_serial(command.spare_serial)?;
    command.ensure_distinct_resolved_serials(primary_serial, spare_serial)?;
    let setup_probe = SecretStorageSetupProbe::expected();
    let setup_inspection = storage_port.inspect_secret_storage_setup(spare_serial, &setup_probe)?;
    let setup_intent = SecretStorageSetupIntent::from_inspection(setup_inspection)?;
    storage_port.initialize_secret_storage(spare_serial, setup_intent.clone())?;
    let spare_pin = if pin_policy.device_requires_pin(spare_serial)? {
        let pin = process.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };
    if let Some(pin) = spare_pin.as_ref() {
        storage_port.verify_pin_input(spare_serial, pin)?;
    }
    for (storage, value) in document.storage_entries(spare_serial) {
        let intent = SecretStorageWriteIntent::initial_enroll_store(storage, value.len())?;
        storage_port.store_secret(spare_serial, intent, value)?;
    }
    storage_port.finalize_secret_storage_setup(spare_serial, setup_intent)?;
    for storage in SecretStorageVerificationPlan::for_serial(spare_serial).into_targets() {
        let inspection = storage_port.inspect_secret_storage_read(spare_serial, &storage)?;
        let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
        let secret = storage_port
            .load_secret(spare_serial, &intent, spare_pin.as_ref())
            .map_err(|error| intent.decode_error(error))?;
        intent.validate_loaded_secret(&secret)?;
    }
    report.write_enroll_report(&EnrollSummary::spare_completed(spare_serial))
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{
            commands::EnrollSpareCommand,
            manifest::SecretManifest,
            piv::{PivApplicationVersion, SecretName},
            storage::{SecretStorageReadInspection, SecretStorageSetupInspection},
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_enroll_spare_with_prompt;

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn setup_inspection() -> SecretStorageSetupInspection {
        SecretStorageSetupInspection {
            key_exists: false,
            piv_version: PivApplicationVersion::minimum_for_secret_storage(),
            pin_retries: 3,
            manifest_bytes: None,
            occupied_object_ids: Vec::new(),
        }
    }

    fn read_inspection() -> SecretStorageReadInspection {
        SecretStorageReadInspection {
            manifest_bytes: Some(SecretManifest::expected().encode().expect("manifest")),
            encoded: Some(vec![1]),
        }
    }

    #[test]
    fn enroll_spare_prompt_rejects_same_requested_serials_before_ports() {
        let mut primary_device = ports::MockDeviceSerialPort::new();
        primary_device.expect_resolve_device_serial().times(0);
        let mut spare_device = ports::MockSpareDeviceSerialPort::new();
        spare_device.expect_resolve_spare_device_serial().times(0);
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy.expect_device_requires_pin().times(0);
        let process = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        storage.expect_inspect_secret_storage_read().times(0);
        let report = ports::MockReportPort::new();

        let result = run_enroll_spare_with_prompt(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2001),
            },
            &mut primary_device,
            &mut spare_device,
            &mut pin_policy,
            &process,
            &mut storage,
            &report,
        );

        assert!(
            result.is_err(),
            "same requested serials must stop before ports"
        );
    }

    #[test]
    fn enroll_spare_prompt_reads_primary_before_spare_setup() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut primary_device = ports::MockDeviceSerialPort::new();
        primary_device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(2)
            .returning(|_| Ok(false));
        let process = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        storage.expect_verify_pin_input().times(0);
        for name in [
            SecretName::BwEmail,
            SecretName::BwPassword,
            SecretName::BwsAccessToken,
        ] {
            storage
                .expect_inspect_secret_storage_read()
                .times(1)
                .in_sequence(&mut sequence)
                .withf(move |serial, storage| *serial == 2001 && storage.name == name)
                .returning(|_, _| Ok(read_inspection()));
            storage
                .expect_load_secret()
                .times(1)
                .in_sequence(&mut sequence)
                .returning(|_, intent, _| {
                    Ok(match intent.storage.name {
                        SecretName::BwEmail => material(b"email"),
                        SecretName::BwPassword => material(b"password"),
                        SecretName::BwsAccessToken => material(b"token"),
                    })
                });
        }
        let mut spare_device = ports::MockSpareDeviceSerialPort::new();
        spare_device
            .expect_resolve_spare_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2002));
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(setup_inspection()));
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .returning(|_, _| Ok(()));
        storage
            .expect_store_secret()
            .times(3)
            .returning(|_, _, _| Ok(()));
        storage
            .expect_finalize_secret_storage_setup()
            .times(1)
            .returning(|_, _| Ok(()));
        for name in [
            SecretName::BwEmail,
            SecretName::BwPassword,
            SecretName::BwsAccessToken,
        ] {
            storage
                .expect_inspect_secret_storage_read()
                .times(1)
                .withf(move |serial, storage| *serial == 2002 && storage.name == name)
                .returning(|_, _| Ok(read_inspection()));
            storage
                .expect_load_secret()
                .times(1)
                .returning(|_, intent, _| {
                    Ok(match intent.storage.name {
                        SecretName::BwEmail => material(b"email"),
                        SecretName::BwPassword => material(b"password"),
                        SecretName::BwsAccessToken => material(b"token"),
                    })
                });
        }
        let mut report = ports::MockReportPort::new();
        report
            .expect_write_enroll_report()
            .times(1)
            .returning(|_| Ok(()));

        run_enroll_spare_with_prompt(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2002),
            },
            &mut primary_device,
            &mut spare_device,
            &mut pin_policy,
            &process,
            &mut storage,
            &report,
        )
    }

    #[test]
    fn enroll_spare_prompt_stops_when_spare_setup_initialization_fails() {
        let mut primary_device = ports::MockDeviceSerialPort::new();
        primary_device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Ok(2001));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(false));
        let process = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        storage.expect_verify_pin_input().times(0);
        for name in [
            SecretName::BwEmail,
            SecretName::BwPassword,
            SecretName::BwsAccessToken,
        ] {
            storage
                .expect_inspect_secret_storage_read()
                .times(1)
                .withf(move |serial, storage| *serial == 2001 && storage.name == name)
                .returning(|_, _| Ok(read_inspection()));
            storage
                .expect_load_secret()
                .times(1)
                .returning(|_, intent, _| {
                    Ok(match intent.storage.name {
                        SecretName::BwEmail => material(b"email"),
                        SecretName::BwPassword => material(b"password"),
                        SecretName::BwsAccessToken => material(b"token"),
                    })
                });
        }
        let mut spare_device = ports::MockSpareDeviceSerialPort::new();
        spare_device
            .expect_resolve_spare_device_serial()
            .times(1)
            .returning(|_| Ok(2002));
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .returning(|_, _| Ok(setup_inspection()));
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .returning(|_, _| Err(anyhow::anyhow!("setup failed")));
        storage.expect_store_secret().times(0);
        storage.expect_finalize_secret_storage_setup().times(0);
        let report = ports::MockReportPort::new();

        let result = run_enroll_spare_with_prompt(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2002),
            },
            &mut primary_device,
            &mut spare_device,
            &mut pin_policy,
            &process,
            &mut storage,
            &report,
        );

        assert!(
            result.is_err(),
            "spare setup failure must stop before store"
        );
    }

    #[test]
    fn enroll_spare_prompt_rejects_invalid_spare_pin_before_writes() {
        let mut primary_device = ports::MockDeviceSerialPort::new();
        primary_device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Ok(2001));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(2)
            .returning(|serial| Ok(serial == 2002));
        let mut process = ports::MockPinInputPort::new();
        process
            .expect_read_pin()
            .times(1)
            .returning(|| Ok(material(b"12")));
        let mut storage = ports::MockSecretStoragePort::new();
        for name in [
            SecretName::BwEmail,
            SecretName::BwPassword,
            SecretName::BwsAccessToken,
        ] {
            storage
                .expect_inspect_secret_storage_read()
                .times(1)
                .withf(move |serial, storage| *serial == 2001 && storage.name == name)
                .returning(|_, _| Ok(read_inspection()));
            storage
                .expect_load_secret()
                .times(1)
                .returning(|_, intent, _| {
                    Ok(match intent.storage.name {
                        SecretName::BwEmail => material(b"email"),
                        SecretName::BwPassword => material(b"password"),
                        SecretName::BwsAccessToken => material(b"token"),
                    })
                });
        }
        let mut spare_device = ports::MockSpareDeviceSerialPort::new();
        spare_device
            .expect_resolve_spare_device_serial()
            .times(1)
            .returning(|_| Ok(2002));
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .returning(|_, _| Ok(setup_inspection()));
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .returning(|_, _| Ok(()));
        storage.expect_verify_pin_input().times(0);
        storage.expect_store_secret().times(0);
        storage.expect_finalize_secret_storage_setup().times(0);
        let report = ports::MockReportPort::new();

        let result = run_enroll_spare_with_prompt(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2002),
            },
            &mut primary_device,
            &mut spare_device,
            &mut pin_policy,
            &process,
            &mut storage,
            &report,
        );

        assert!(result.is_err(), "invalid spare PIN must stop before writes");
    }

    #[test]
    fn enroll_spare_prompt_rejects_wrong_spare_pin_before_writes() {
        let mut primary_device = ports::MockDeviceSerialPort::new();
        primary_device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Ok(2001));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(2)
            .returning(|serial| Ok(serial == 2002));
        let mut process = ports::MockPinInputPort::new();
        process
            .expect_read_pin()
            .times(1)
            .returning(|| Ok(material(b"123456")));
        let mut storage = ports::MockSecretStoragePort::new();
        for name in [
            SecretName::BwEmail,
            SecretName::BwPassword,
            SecretName::BwsAccessToken,
        ] {
            storage
                .expect_inspect_secret_storage_read()
                .times(1)
                .withf(move |serial, storage| *serial == 2001 && storage.name == name)
                .returning(|_, _| Ok(read_inspection()));
            storage
                .expect_load_secret()
                .times(1)
                .returning(|_, intent, _| {
                    Ok(match intent.storage.name {
                        SecretName::BwEmail => material(b"email"),
                        SecretName::BwPassword => material(b"password"),
                        SecretName::BwsAccessToken => material(b"token"),
                    })
                });
        }
        let mut spare_device = ports::MockSpareDeviceSerialPort::new();
        spare_device
            .expect_resolve_spare_device_serial()
            .times(1)
            .returning(|_| Ok(2002));
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .returning(|_, _| Ok(setup_inspection()));
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .returning(|_, _| Ok(()));
        storage
            .expect_verify_pin_input()
            .times(1)
            .withf(|serial, _| *serial == 2002)
            .returning(|_, _| Err(anyhow::anyhow!("PIN verify failed")));
        storage.expect_store_secret().times(0);
        storage.expect_finalize_secret_storage_setup().times(0);
        let report = ports::MockReportPort::new();

        let result = run_enroll_spare_with_prompt(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2002),
            },
            &mut primary_device,
            &mut spare_device,
            &mut pin_policy,
            &process,
            &mut storage,
            &report,
        );

        assert!(result.is_err(), "wrong spare PIN must stop before writes");
    }
}
