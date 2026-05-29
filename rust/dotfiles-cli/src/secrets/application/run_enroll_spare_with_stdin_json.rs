//! enroll-spare(stdin-json) の順序を固定し、device 衝突停止条件を入力方式に依存させない。

use crate::Result;
use crate::secrets::{
    domain::{
        command::EnrollSpareCommand,
        manifest::BootstrapSecretDocument,
        piv::validate_piv_pin_len,
        storage::{
            SecretStorageReadIntent, SecretStorageSetupIntent, SecretStorageSetupProbe,
            SecretStorageVerificationPlan, SecretStorageWriteIntent,
        },
        summary::EnrollSummary,
    },
    ports,
};

/// stdin JSON document で spare YubiKey に bootstrap secret 一式を登録する。
///
/// primary と spare の衝突停止条件を先に評価し、device 選択・入力実装は port に委譲して
/// use case の順序責務だけを保持する。
pub(crate) fn run_enroll_spare_with_stdin_json<D, I, P, S, R>(
    command: EnrollSpareCommand,
    spare_device: &mut D,
    pin_policy: &mut impl ports::DevicePinPolicyPort,
    document_input: &I,
    pin_input: &P,
    storage_port: &mut S,
    report: &R,
) -> Result<()>
where
    D: ports::SpareDeviceSerialPort,
    I: ports::BootstrapSecretDocumentInputPort,
    P: ports::PinInputPort,
    S: ports::SecretStoragePort,
    R: ports::ReportPort,
{
    command.ensure_requested_serials_distinct()?;
    let spare_serial = spare_device.resolve_spare_device_serial(command.spare_serial)?;
    command.ensure_requested_primary_differs_from_spare(spare_serial)?;
    let setup_probe = SecretStorageSetupProbe::expected();
    let setup_inspection = storage_port.inspect_secret_storage_setup(spare_serial, &setup_probe)?;
    let setup_intent = SecretStorageSetupIntent::from_inspection(setup_inspection)?;
    let fields = document_input.read_bootstrap_secret_fields()?;
    let document = BootstrapSecretDocument::from_field_map(fields)?;
    storage_port.initialize_secret_storage(spare_serial, setup_intent.clone())?;
    for (storage, value) in document.storage_entries(spare_serial) {
        let intent = SecretStorageWriteIntent::initial_enroll_store(storage, value.len())?;
        storage_port.store_secret(spare_serial, intent, value)?;
    }
    storage_port.finalize_secret_storage_setup(spare_serial, setup_intent)?;
    let pin = if pin_policy.device_requires_pin(spare_serial)? {
        let pin = pin_input.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };
    for storage in SecretStorageVerificationPlan::for_serial(spare_serial).into_targets() {
        let inspection = storage_port.inspect_secret_storage_read(spare_serial, &storage)?;
        let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
        let secret = storage_port
            .load_secret(spare_serial, &intent, pin.as_ref())
            .map_err(|error| intent.decode_error(error))?;
        intent.validate_loaded_secret(&secret)?;
    }
    report.write_enroll_report(&EnrollSummary::spare_completed(spare_serial))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::secrets::{
        domain::{
            command::EnrollSpareCommand,
            manifest::SecretManifest,
            piv::{PivApplicationVersion, SecretName},
            storage::{SecretStorageReadInspection, SecretStorageSetupInspection},
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_enroll_spare_with_stdin_json;

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn fields() -> BTreeMap<String, ProtectedSecret> {
        [
            ("bw-email".to_owned(), material(b"email")),
            ("bw-password".to_owned(), material(b"password")),
            ("bws-access-token".to_owned(), material(b"token")),
        ]
        .into_iter()
        .collect()
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
    fn enroll_spare_stdin_json_reads_pin_only_when_required() -> crate::Result<()> {
        let mut spare_device = ports::MockSpareDeviceSerialPort::new();
        spare_device
            .expect_resolve_spare_device_serial()
            .times(1)
            .returning(|_| Ok(2002));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(true));
        let mut document_input = ports::MockBootstrapSecretDocumentInputPort::new();
        document_input
            .expect_read_bootstrap_secret_fields()
            .times(1)
            .returning(|| Ok(fields()));
        let mut pin_input = ports::MockPinInputPort::new();
        pin_input
            .expect_read_pin()
            .times(1)
            .returning(|| Ok(material(b"123456")));
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
                .withf(move |serial, storage| *serial == 2002 && storage.name == name)
                .returning(|_, _| Ok(read_inspection()));
            storage
                .expect_load_secret()
                .times(1)
                .withf(move |_, intent, pin| intent.storage.name == name && pin.is_some())
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

        run_enroll_spare_with_stdin_json(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2002),
            },
            &mut spare_device,
            &mut pin_policy,
            &document_input,
            &pin_input,
            &mut storage,
            &report,
        )
    }

    #[test]
    fn enroll_spare_stdin_json_rejects_same_requested_serials_before_ports() {
        let mut spare_device = ports::MockSpareDeviceSerialPort::new();
        spare_device.expect_resolve_spare_device_serial().times(0);
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy.expect_device_requires_pin().times(0);
        let mut document_input = ports::MockBootstrapSecretDocumentInputPort::new();
        document_input
            .expect_read_bootstrap_secret_fields()
            .times(0);
        let pin_input = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        storage.expect_inspect_secret_storage_setup().times(0);
        let report = ports::MockReportPort::new();

        let result = run_enroll_spare_with_stdin_json(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2001),
            },
            &mut spare_device,
            &mut pin_policy,
            &document_input,
            &pin_input,
            &mut storage,
            &report,
        );

        assert!(
            result.is_err(),
            "same requested serials must stop before ports"
        );
    }

    #[test]
    fn enroll_spare_stdin_json_rejects_resolved_spare_collision_before_setup() {
        let mut spare_device = ports::MockSpareDeviceSerialPort::new();
        spare_device
            .expect_resolve_spare_device_serial()
            .times(1)
            .returning(|_| Ok(2001));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy.expect_device_requires_pin().times(0);
        let mut document_input = ports::MockBootstrapSecretDocumentInputPort::new();
        document_input
            .expect_read_bootstrap_secret_fields()
            .times(0);
        let pin_input = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        storage.expect_inspect_secret_storage_setup().times(0);
        let report = ports::MockReportPort::new();

        let result = run_enroll_spare_with_stdin_json(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: None,
            },
            &mut spare_device,
            &mut pin_policy,
            &document_input,
            &pin_input,
            &mut storage,
            &report,
        );

        assert!(
            result.is_err(),
            "resolved spare collision must stop before setup"
        );
    }

    #[test]
    fn enroll_spare_stdin_json_stops_when_setup_initialization_fails() {
        let mut spare_device = ports::MockSpareDeviceSerialPort::new();
        spare_device
            .expect_resolve_spare_device_serial()
            .times(1)
            .returning(|_| Ok(2002));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy.expect_device_requires_pin().times(0);
        let mut document_input = ports::MockBootstrapSecretDocumentInputPort::new();
        document_input
            .expect_read_bootstrap_secret_fields()
            .times(1)
            .returning(|| Ok(fields()));
        let pin_input = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
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

        let result = run_enroll_spare_with_stdin_json(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2002),
            },
            &mut spare_device,
            &mut pin_policy,
            &document_input,
            &pin_input,
            &mut storage,
            &report,
        );

        assert!(result.is_err(), "setup failure must stop before store");
    }
}
