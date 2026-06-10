//! enroll-primary(prompt) の順序を固定し、入力 I/O 変更を storage 手順から分離して誤登録を防ぐ。

use crate::Result;
use crate::secrets::{
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
/// 入力手段の詳細は `SecretInputPort` 側へ閉じ込める。use case は単一接続確認と setup を secret 入力前に
/// 完了し、setup 不能な YubiKey では bootstrap secret を読まずに停止する。
pub(crate) async fn run_enroll_primary_with_prompt<D, I, P, S, R, B>(
    command: EnrollPrimaryCommand,
    device_serial: &mut D,
    pin_policy: &mut impl ports::DevicePinPolicyPort,
    secret_input: &I,
    pin_input: &P,
    storage_port: &mut S,
    report: &R,
    bws_client: &B,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    I: ports::SecretInputPort,
    P: ports::PinInputPort,
    S: ports::SecretStoragePort,
    R: ports::ReportPort,
    B: ports::BwsClientPort,
{
    let _ = command;
    let serial = device_serial.resolve_device_serial()?;
    let setup_probe = SecretStorageSetupProbe::expected();
    let setup_inspection = storage_port.inspect_secret_storage_setup(serial, &setup_probe)?;
    let setup_intent = SecretStorageSetupIntent::from_inspection(setup_inspection)?;
    storage_port.initialize_secret_storage(serial, setup_intent.clone())?;
    let bw_email = secret_input.read_bw_email_secret()?;
    let bw_password = secret_input.read_bw_password_secret()?;
    let bws_access_token = secret_input.read_bws_access_token_secret()?;
    bws_client
        .ensure_recovery_token_provenance(&bws_access_token)
        .await?;
    let document =
        BootstrapSecretDocument::from_secret_materials(&bw_email, &bw_password, &bws_access_token)?;
    for (storage, value) in document.storage_entries(serial) {
        let intent = SecretStorageWriteIntent::initial_enroll_store(storage, value.len())?;
        storage_port.store_secret(serial, intent, value)?;
    }
    storage_port.finalize_secret_storage_setup(serial, setup_intent)?;
    let pin = if pin_policy.device_requires_pin(serial)? {
        let pin = pin_input.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };
    for storage in SecretStorageVerificationPlan::for_serial(serial).into_targets() {
        let inspection = storage_port.inspect_secret_storage_read(serial, &storage)?;
        let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
        let secret = storage_port
            .load_secret(serial, &intent, pin.as_ref())
            .map_err(|error| intent.decode_error(error))?;
        intent.validate_loaded_secret(&secret)?;
    }
    report.write_enroll_report(&EnrollSummary::primary_completed())
}

#[cfg(test)]
mod tests {
    use crate::secrets::{
        domain::{
            commands::EnrollPrimaryCommand,
            manifest::SecretManifest,
            piv::{PivApplicationVersion, SecretName},
            storage::{SecretStorageReadInspection, SecretStorageSetupInspection},
            verification::{CheckName, CheckStatus},
        },
        ports,
        ports::ProtectedSecret,
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

    fn expect_bws_recovery_token_gate(
        bws: &mut ports::MockBwsClientPort,
        sequence: &mut mockall::Sequence,
        outcome: crate::Result<()>,
    ) {
        let mut outcome = Some(outcome);
        bws.expect_ensure_recovery_token_provenance()
            .times(1)
            .in_sequence(sequence)
            .returning(move |_| outcome.take().expect("single use outcome"));
    }

    #[tokio::test]
    async fn enroll_primary_prompt_stores_verifies_and_reports() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(setup_inspection()));
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
            .expect_read_bws_access_token_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(b"recovery-token")));
        let mut bws = ports::MockBwsClientPort::new();
        expect_bws_recovery_token_gate(&mut bws, &mut sequence, Ok(()));
        storage
            .expect_store_secret()
            .times(3)
            .in_sequence(&mut sequence)
            .returning(|_, _, _| Ok(()));
        storage
            .expect_finalize_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(false));
        let pin_input = ports::MockPinInputPort::new();
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
                .withf(move |serial, intent, _| *serial == 2001 && intent.storage.name == name)
                .returning(|_, intent, _| {
                    Ok(match intent.storage.name {
                        SecretName::BwEmail => material(b"email"),
                        SecretName::BwPassword => material(b"password"),
                        SecretName::BwsAccessToken => material(b"recovery-token"),
                    })
                });
        }
        let mut report = ports::MockReportPort::new();
        report
            .expect_write_enroll_report()
            .times(1)
            .withf(|summary| summary.checks.get(&CheckName::LocalStorage) == Some(&CheckStatus::Ok))
            .returning(|_| Ok(()));

        run_enroll_primary_with_prompt(
            EnrollPrimaryCommand,
            &mut device_serial,
            &mut pin_policy,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
            &bws,
        )
        .await
    }

    #[tokio::test]
    async fn enroll_primary_prompt_stops_when_setup_inspection_fails() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2001));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy.expect_device_requires_pin().times(0);
        let mut secret_input = ports::MockSecretInputPort::new();
        secret_input.expect_read_bw_email_secret().times(0);
        let pin_input = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        let bws = ports::MockBwsClientPort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .returning(|_, _| Err(anyhow::anyhow!("setup inspect failed")));
        storage.expect_initialize_secret_storage().times(0);
        let report = ports::MockReportPort::new();

        let result = run_enroll_primary_with_prompt(
            EnrollPrimaryCommand,
            &mut device_serial,
            &mut pin_policy,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
            &bws,
        )
        .await;

        assert!(result.is_err(), "setup failure must stop before input");
    }

    #[tokio::test]
    async fn enroll_primary_prompt_stops_when_initialize_fails_before_input() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2001));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy.expect_device_requires_pin().times(0);
        let mut secret_input = ports::MockSecretInputPort::new();
        secret_input.expect_read_bw_email_secret().times(0);
        secret_input.expect_read_bw_password_secret().times(0);
        secret_input.expect_read_bws_access_token_secret().times(0);
        let pin_input = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        let bws = ports::MockBwsClientPort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .returning(|_, _| Ok(setup_inspection()));
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .returning(|_, _| Err(anyhow::anyhow!("initialize failed")));
        storage.expect_store_secret().times(0);
        let report = ports::MockReportPort::new();

        let result = run_enroll_primary_with_prompt(
            EnrollPrimaryCommand,
            &mut device_serial,
            &mut pin_policy,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
            &bws,
        )
        .await;

        assert!(result.is_err(), "initialize failure must stop before input");
    }

    #[tokio::test]
    async fn enroll_primary_prompt_reads_pin_when_required() -> crate::Result<()> {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
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
                        SecretName::BwsAccessToken => material(b"recovery-token"),
                    })
                });
        }
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(true));
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
            .expect_read_bws_access_token_secret()
            .times(1)
            .returning(|| Ok(material(b"recovery-token")));
        let mut bws = ports::MockBwsClientPort::new();
        let mut sequence = mockall::Sequence::new();
        expect_bws_recovery_token_gate(&mut bws, &mut sequence, Ok(()));
        let mut pin_input = ports::MockPinInputPort::new();
        pin_input
            .expect_read_pin()
            .times(1)
            .returning(|| Ok(material(b"123456")));
        let mut report = ports::MockReportPort::new();
        report
            .expect_write_enroll_report()
            .times(1)
            .returning(|_| Ok(()));

        run_enroll_primary_with_prompt(
            EnrollPrimaryCommand,
            &mut device_serial,
            &mut pin_policy,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
            &bws,
        )
        .await
    }

    #[tokio::test]
    async fn enroll_primary_prompt_stops_when_store_fails() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .returning(|_, _| Ok(setup_inspection()));
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .returning(|_, _| Ok(()));
        storage
            .expect_store_secret()
            .times(1)
            .returning(|_, _, _| Err(anyhow::anyhow!("store failed")));
        storage.expect_finalize_secret_storage_setup().times(0);
        storage.expect_inspect_secret_storage_read().times(0);
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy.expect_device_requires_pin().times(0);
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
            .expect_read_bws_access_token_secret()
            .times(1)
            .returning(|| Ok(material(b"recovery-token")));
        let mut bws = ports::MockBwsClientPort::new();
        let mut sequence = mockall::Sequence::new();
        expect_bws_recovery_token_gate(&mut bws, &mut sequence, Ok(()));
        let pin_input = ports::MockPinInputPort::new();
        let mut report = ports::MockReportPort::new();
        report.expect_write_enroll_report().times(0);

        let result = run_enroll_primary_with_prompt(
            EnrollPrimaryCommand,
            &mut device_serial,
            &mut pin_policy,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
            &bws,
        )
        .await;

        assert!(result.is_err(), "store failure must stop before verify");
    }

    #[tokio::test]
    async fn enroll_primary_prompt_rejects_same_token_before_store() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .returning(|_, _| Ok(setup_inspection()));
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .returning(|_, _| Ok(()));
        storage.expect_store_secret().times(0);
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy.expect_device_requires_pin().times(0);
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
            .expect_read_bws_access_token_secret()
            .times(1)
            .returning(|| Ok(material(b"same-token")));
        let mut bws = ports::MockBwsClientPort::new();
        let mut sequence = mockall::Sequence::new();
        expect_bws_recovery_token_gate(
            &mut bws,
            &mut sequence,
            Err(anyhow::anyhow!(
                "refusing to store bws-access-token: recovery token must differ from the provisioning token"
            )),
        );
        let pin_input = ports::MockPinInputPort::new();
        let report = ports::MockReportPort::new();

        let result = run_enroll_primary_with_prompt(
            EnrollPrimaryCommand,
            &mut device_serial,
            &mut pin_policy,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
            &bws,
        )
        .await;

        assert_eq!(
            result
                .expect_err("same provisioning token must be rejected")
                .to_string(),
            "refusing to store bws-access-token: recovery token must differ from the provisioning token"
        );
    }

    #[tokio::test]
    async fn enroll_primary_prompt_rejects_missing_or_invalid_provenance_before_store() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .returning(|_, _| Ok(setup_inspection()));
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .returning(|_, _| Ok(()));
        storage.expect_store_secret().times(0);
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy.expect_device_requires_pin().times(0);
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
            .expect_read_bws_access_token_secret()
            .times(1)
            .returning(|| Ok(material(b"candidate-token")));
        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_ensure_recovery_token_provenance()
            .times(1)
            .returning(|_| {
                Err(anyhow::anyhow!(
                    "refusing to store bws-access-token: password-store-remote is missing provisioning token provenance"
                ))
            });
        let pin_input = ports::MockPinInputPort::new();
        let report = ports::MockReportPort::new();

        let result = run_enroll_primary_with_prompt(
            EnrollPrimaryCommand,
            &mut device_serial,
            &mut pin_policy,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
            &bws,
        )
        .await;

        assert_eq!(
            result
                .expect_err("tampered provenance note must be rejected")
                .to_string(),
            "refusing to store bws-access-token: password-store-remote is missing provisioning token provenance"
        );
    }

    #[tokio::test]
    async fn enroll_primary_prompt_stops_when_verify_load_fails() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
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
            .expect_read_bws_access_token_secret()
            .times(1)
            .returning(|| Ok(material(b"recovery-token")));
        let mut bws = ports::MockBwsClientPort::new();
        let mut sequence = mockall::Sequence::new();
        expect_bws_recovery_token_gate(&mut bws, &mut sequence, Ok(()));
        let pin_input = ports::MockPinInputPort::new();
        let mut report = ports::MockReportPort::new();
        report.expect_write_enroll_report().times(0);

        let result = run_enroll_primary_with_prompt(
            EnrollPrimaryCommand,
            &mut device_serial,
            &mut pin_policy,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
            &bws,
        )
        .await;

        assert!(result.is_err(), "verify failure must stop before report");
    }
}
