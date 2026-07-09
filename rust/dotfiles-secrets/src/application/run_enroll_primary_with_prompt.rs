//! enroll-primary(prompt) の順序を固定し、入力 I/O 変更を storage 手順から分離して誤登録を防ぐ。

use crate::Result;
use crate::{
    domain::{
        commands::EnrollPrimaryCommand,
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

/// prompt 入力で primary YubiKey に bootstrap secret 一式を登録する。
///
/// 入力手段の詳細は `SecretInputPort` 側へ閉じ込め、use case は setup→store→verify の
/// 順序制御だけを担って application 層の責務境界を維持する。
pub(crate) fn run_enroll_primary_with_prompt<D, I, P, S, R>(
    command: EnrollPrimaryCommand,
    device_serial: &mut D,
    pin_policy: &mut impl ports::DevicePinPolicyPort,
    secret_input: &I,
    pin_input: &P,
    storage_port: &mut S,
    report: &R,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    I: ports::SecretInputPort,
    P: ports::PinInputPort,
    S: ports::SecretStoragePort,
    R: ports::ReportPort,
{
    let serial = device_serial.resolve_device_serial(command.serial)?;
    let setup_probe = SecretStorageSetupProbe::expected();
    let setup_inspection = storage_port.inspect_secret_storage_setup(serial, &setup_probe)?;
    let setup_intent = SecretStorageSetupIntent::from_inspection(setup_inspection)?;
    let pin = if pin_policy.device_requires_pin(serial)? {
        let pin = pin_input.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };
    if let Some(pin) = pin.as_ref() {
        storage_port.verify_pin_input(serial, pin)?;
    }
    storage_port.initialize_secret_storage(serial, setup_intent.clone())?;
    let bw_email = secret_input.read_bw_email_secret()?;
    let bw_password = secret_input.read_bw_password_secret()?;
    let bitwarden_client_id = secret_input.read_bitwarden_client_id_secret()?;
    let bitwarden_client_secret = secret_input.read_bitwarden_client_secret_secret()?;
    let document =
        BootstrapSecretDocument::from_secret_materials(&bw_email, &bw_password, &bitwarden_client_id, &bitwarden_client_secret)?;
    for (storage, value) in document.storage_entries(serial) {
        let intent = SecretStorageWriteIntent::initial_enroll_store(storage, value.len())?;
        storage_port.store_secret(serial, intent, value)?;
    }
    storage_port.finalize_secret_storage_setup(serial, setup_intent)?;
    for storage in SecretStorageVerificationPlan::for_serial(serial).into_targets() {
        let inspection = storage_port.inspect_secret_storage_read(serial, &storage)?;
        let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
        let secret = storage_port
            .load_secret(serial, &intent, pin.as_ref())
            .map_err(|error| intent.decode_error(error))?;
        intent.validate_loaded_secret(&secret)?;
    }
    report.write_enroll_report(&EnrollSummary::primary_completed(serial))
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{
            commands::EnrollPrimaryCommand,
            manifest::SecretManifest,
            piv::{PivApplicationVersion, SecretName},
            storage::{SecretStorageReadInspection, SecretStorageSetupInspection},
            verification::{CheckName, CheckStatus},
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_enroll_primary_with_prompt;

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
    fn enroll_primary_prompt_stores_verifies_and_reports() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(setup_inspection()));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(false));
        storage.expect_verify_pin_input().times(0);
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));
        let mut secret_input = ports::MockSecretInputPort::new();
        secret_input
            .expect_read_bw_email_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(b"email")));
        secret_input
            .expect_read_bw_password_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(b"password")));
        secret_input
            .expect_read_bitwarden_client_id_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(b"client-id")));
        secret_input
            .expect_read_bitwarden_client_secret_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(b"client-secret")));
        storage
            .expect_store_secret()
            .times(4)
            .in_sequence(&mut sequence)
            .returning(|_, _, _| Ok(()));
        storage
            .expect_finalize_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));
        let pin_input = ports::MockPinInputPort::new();
        for name in [
            SecretName::BwEmail,
            SecretName::BwPassword,
            SecretName::BitwardenClientId,
            SecretName::BitwardenClientSecret,
        ] {
            storage
                .expect_inspect_secret_storage_read()
                .times(1)
                .withf(move |serial, storage| *serial == 2001 && storage.name == name)
                .returning(|_, _| Ok(read_inspection()));
            storage
                .expect_load_secret()
                .times(1)
                .withf(move |serial, intent, _| *serial == 2001 && intent.storage.name == name)
                .returning(|_, intent, _| {
                    Ok(match intent.storage.name {
                        SecretName::BwEmail => material(b"email"),
                        SecretName::BwPassword => material(b"password"),
                        SecretName::BitwardenClientId | SecretName::BitwardenClientSecret => material(b"token"),
                    })
                });
        }
        let mut report = ports::MockReportPort::new();
        report
            .expect_write_enroll_report()
            .times(1)
            .withf(|summary| {
                summary.serial == 2001
                    && summary.checks.get(&CheckName::LocalStorage) == Some(&CheckStatus::Ok)
            })
            .returning(|_| Ok(()));

        run_enroll_primary_with_prompt(
            EnrollPrimaryCommand { serial: Some(2001) },
            &mut device_serial,
            &mut pin_policy,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
        )
    }

    #[test]
    fn enroll_primary_prompt_stops_when_setup_inspection_fails() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Ok(2001));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(0)
            .returning(|_| Ok(false));
        let mut secret_input = ports::MockSecretInputPort::new();
        secret_input.expect_read_bw_email_secret().times(0);
        let pin_input = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .returning(|_, _| Err(anyhow::anyhow!("setup inspect failed")));
        storage.expect_initialize_secret_storage().times(0);
        let report = ports::MockReportPort::new();

        let result = run_enroll_primary_with_prompt(
            EnrollPrimaryCommand { serial: Some(2001) },
            &mut device_serial,
            &mut pin_policy,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
        );

        assert!(result.is_err(), "setup failure must stop before input");
    }

    #[test]
    fn enroll_primary_prompt_reads_pin_when_required() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(setup_inspection()));
        storage
            .expect_store_secret()
            .times(4)
            .returning(|_, _, _| Ok(()));
        storage
            .expect_finalize_secret_storage_setup()
            .times(1)
            .returning(|_, _| Ok(()));
        for name in [
            SecretName::BwEmail,
            SecretName::BwPassword,
            SecretName::BitwardenClientId,
            SecretName::BitwardenClientSecret,
        ] {
            storage
                .expect_inspect_secret_storage_read()
                .times(1)
                .withf(move |_, storage| storage.name == name)
                .returning(|_, _| Ok(read_inspection()));
            storage
                .expect_load_secret()
                .times(1)
                .withf(move |_, intent, pin| intent.storage.name == name && pin.is_some())
                .returning(|_, intent, _| {
                    Ok(match intent.storage.name {
                        SecretName::BwEmail => material(b"email"),
                        SecretName::BwPassword => material(b"password"),
                        SecretName::BitwardenClientId | SecretName::BitwardenClientSecret => material(b"token"),
                    })
                });
        }
        let mut secret_input = ports::MockSecretInputPort::new();
        secret_input
            .expect_read_bw_email_secret()
            .times(1)
            .returning(|| Ok(material(b"email")));
        secret_input
            .expect_read_bw_password_secret()
            .times(1)
            .returning(|| Ok(material(b"password")));
        secret_input
            .expect_read_bitwarden_client_id_secret()
            .times(1)
            .returning(|| Ok(material(b"client-id")));
        secret_input
            .expect_read_bitwarden_client_secret_secret()
            .times(1)
            .returning(|| Ok(material(b"client-secret")));
        let mut pin_input = ports::MockPinInputPort::new();
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(true));
        pin_input
            .expect_read_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(b"123456")));
        storage
            .expect_verify_pin_input()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));
        let mut report = ports::MockReportPort::new();
        report
            .expect_write_enroll_report()
            .times(1)
            .returning(|_| Ok(()));

        run_enroll_primary_with_prompt(
            EnrollPrimaryCommand { serial: Some(2001) },
            &mut device_serial,
            &mut pin_policy,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
        )
    }

    #[test]
    fn enroll_primary_prompt_rejects_invalid_pin_before_secret_reads() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .returning(|_, _| Ok(setup_inspection()));
        storage.expect_initialize_secret_storage().times(0);
        storage.expect_verify_pin_input().times(0);
        storage.expect_store_secret().times(0);
        storage.expect_finalize_secret_storage_setup().times(0);
        storage.expect_inspect_secret_storage_read().times(0);
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(true));
        let mut pin_input = ports::MockPinInputPort::new();
        pin_input
            .expect_read_pin()
            .times(1)
            .returning(|| Ok(material(b"12")));
        let mut secret_input = ports::MockSecretInputPort::new();
        secret_input.expect_read_bw_email_secret().times(0);
        secret_input.expect_read_bw_password_secret().times(0);
        secret_input.expect_read_bitwarden_client_id_secret().times(0);
        secret_input.expect_read_bitwarden_client_secret_secret().times(0);
        let report = ports::MockReportPort::new();

        let result = run_enroll_primary_with_prompt(
            EnrollPrimaryCommand { serial: Some(2001) },
            &mut device_serial,
            &mut pin_policy,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
        );

        assert!(
            result.is_err(),
            "invalid PIN must stop before reading secrets"
        );
    }

    #[test]
    fn enroll_primary_prompt_rejects_wrong_pin_before_secret_reads() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .returning(|_, _| Ok(setup_inspection()));
        storage.expect_initialize_secret_storage().times(0);
        storage
            .expect_verify_pin_input()
            .times(1)
            .returning(|_, _| Err(anyhow::anyhow!("PIN verify failed")));
        storage.expect_store_secret().times(0);
        storage.expect_finalize_secret_storage_setup().times(0);
        storage.expect_inspect_secret_storage_read().times(0);
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(true));
        let mut pin_input = ports::MockPinInputPort::new();
        pin_input
            .expect_read_pin()
            .times(1)
            .returning(|| Ok(material(b"123456")));
        let mut secret_input = ports::MockSecretInputPort::new();
        secret_input.expect_read_bw_email_secret().times(0);
        secret_input.expect_read_bw_password_secret().times(0);
        secret_input.expect_read_bitwarden_client_id_secret().times(0);
        secret_input.expect_read_bitwarden_client_secret_secret().times(0);
        let report = ports::MockReportPort::new();

        let result = run_enroll_primary_with_prompt(
            EnrollPrimaryCommand { serial: Some(2001) },
            &mut device_serial,
            &mut pin_policy,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
        );

        assert!(
            result.is_err(),
            "wrong PIN must stop before reading secrets and writes"
        );
    }

    #[test]
    fn enroll_primary_prompt_stops_when_store_fails() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .returning(|_, _| Ok(setup_inspection()));
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .returning(|_, _| Ok(()));
        storage.expect_verify_pin_input().times(0);
        storage
            .expect_store_secret()
            .times(1)
            .returning(|_, _, _| Err(anyhow::anyhow!("store failed")));
        storage.expect_finalize_secret_storage_setup().times(0);
        storage.expect_inspect_secret_storage_read().times(0);
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(false));
        let mut secret_input = ports::MockSecretInputPort::new();
        secret_input
            .expect_read_bw_email_secret()
            .times(1)
            .returning(|| Ok(material(b"email")));
        secret_input
            .expect_read_bw_password_secret()
            .times(1)
            .returning(|| Ok(material(b"password")));
        secret_input
            .expect_read_bitwarden_client_id_secret()
            .times(1)
            .returning(|| Ok(material(b"client-id")));
        secret_input
            .expect_read_bitwarden_client_secret_secret()
            .times(1)
            .returning(|| Ok(material(b"client-secret")));
        let pin_input = ports::MockPinInputPort::new();
        let mut report = ports::MockReportPort::new();
        report.expect_write_enroll_report().times(0);

        let result = run_enroll_primary_with_prompt(
            EnrollPrimaryCommand { serial: Some(2001) },
            &mut device_serial,
            &mut pin_policy,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
        );

        assert!(result.is_err(), "store failure must stop before verify");
    }

    #[test]
    fn enroll_primary_prompt_stops_when_verify_load_fails() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .returning(|_, _| Ok(setup_inspection()));
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .returning(|_, _| Ok(()));
        storage.expect_verify_pin_input().times(0);
        storage
            .expect_store_secret()
            .times(4)
            .returning(|_, _, _| Ok(()));
        storage
            .expect_finalize_secret_storage_setup()
            .times(1)
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .returning(|_, _| Ok(read_inspection()));
        storage
            .expect_load_secret()
            .times(1)
            .returning(|_, _, _| Err(anyhow::anyhow!("verify failed")));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(false));
        let mut secret_input = ports::MockSecretInputPort::new();
        secret_input
            .expect_read_bw_email_secret()
            .times(1)
            .returning(|| Ok(material(b"email")));
        secret_input
            .expect_read_bw_password_secret()
            .times(1)
            .returning(|| Ok(material(b"password")));
        secret_input
            .expect_read_bitwarden_client_id_secret()
            .times(1)
            .returning(|| Ok(material(b"client-id")));
        secret_input
            .expect_read_bitwarden_client_secret_secret()
            .times(1)
            .returning(|| Ok(material(b"client-secret")));
        let pin_input = ports::MockPinInputPort::new();
        let mut report = ports::MockReportPort::new();
        report.expect_write_enroll_report().times(0);

        let result = run_enroll_primary_with_prompt(
            EnrollPrimaryCommand { serial: Some(2001) },
            &mut device_serial,
            &mut pin_policy,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
        );

        assert!(result.is_err(), "verify failure must stop before report");
    }
}
