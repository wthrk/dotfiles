//! enroll-primary(stdin-json) の順序を固定し、JSON 入力仕様変更を usecase 境界へ逆流させない。

use crate::Result;
use crate::{
    domain::{
        commands::EnrollPrimaryCommand,
        enrollment::EnrollSummary,
        manifest::BootstrapSecretDocument,
        storage::{
            SecretStorageReadIntent, SecretStorageSetupIntent, SecretStorageSetupProbe,
            SecretStorageVerificationPlan, SecretStorageWriteIntent,
        },
    },
    ports,
};

/// stdin JSON document で primary YubiKey に bootstrap secret 一式を登録する。
///
/// JSON parse を use case へ持ち込まず `BootstrapSecretDocumentInputPort` へ委譲し、
/// enrollment 手順のみを application 層で固定する。
pub(crate) fn run_enroll_primary_with_stdin_json<D, I, S, R>(
    command: EnrollPrimaryCommand,
    device_serial: &mut D,
    document_input: &I,
    storage_port: &mut S,
    report: &R,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    I: ports::BootstrapSecretDocumentInputPort,
    S: ports::SecretStoragePort,
    R: ports::ReportPort,
{
    let serial = device_serial.resolve_device_serial(command.serial)?;
    let setup_probe = SecretStorageSetupProbe::expected();
    let setup_inspection = storage_port.inspect_secret_storage_setup(serial, &setup_probe)?;
    let setup_intent = SecretStorageSetupIntent::for_enrollment(setup_inspection)?;
    let public_key_spki = storage_port.initialize_secret_storage(serial, setup_intent.clone())?;
    let fields = document_input.read_bootstrap_secret_fields()?;
    let document = BootstrapSecretDocument::from_field_map(fields)?;
    for (storage, value) in document.storage_entries(serial) {
        let intent = SecretStorageWriteIntent::initial_enroll_store(
            storage,
            value.len(),
            public_key_spki.clone(),
        )?;
        storage_port.store_secret(serial, intent, value)?;
    }
    storage_port.finalize_secret_storage_setup(
        serial,
        setup_intent.manifest_for_public_key(public_key_spki)?,
    )?;
    for storage in SecretStorageVerificationPlan::for_serial(serial).into_targets() {
        let inspection = storage_port.inspect_secret_storage_read(serial, &storage)?;
        let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
        let secret = storage_port
            .load_secret(serial, &intent)
            .map_err(|error| intent.decode_error(error))?;
        intent.validate_loaded_secret(&secret)?;
    }
    report.write_enroll_report(&EnrollSummary::primary_completed(serial))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{
        domain::{
            commands::EnrollPrimaryCommand, piv::PivApplicationVersion,
            storage::SecretStorageSetupInspection,
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_enroll_primary_with_stdin_json;

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn fields() -> BTreeMap<String, ProtectedSecret> {
        [
            ("bw-email".to_owned(), material(b"email")),
            ("bw-password".to_owned(), material(b"password")),
            ("bitwarden-client-id".to_owned(), material(b"client-id")),
            (
                "bitwarden-client-secret".to_owned(),
                material(b"client-secret"),
            ),
        ]
        .into_iter()
        .collect()
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
    fn enroll_primary_stdin_json_stops_before_document_input_when_setup_inspection_fails() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Ok(2001));
        let mut document_input = ports::MockBootstrapSecretDocumentInputPort::new();
        document_input
            .expect_read_bootstrap_secret_fields()
            .times(0);
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .returning(|_, _| Err(anyhow::anyhow!("setup inspect failed")));
        storage.expect_initialize_secret_storage().times(0);
        storage.expect_store_secret().times(0);
        let report = ports::MockReportPort::new();

        let result = run_enroll_primary_with_stdin_json(
            EnrollPrimaryCommand { serial: Some(2001) },
            &mut device_serial,
            &document_input,
            &mut storage,
            &report,
        );

        assert!(
            result.is_err(),
            "setup inspection failure must stop before document input"
        );
    }

    #[test]
    fn enroll_primary_stdin_json_rejects_existing_storage_before_document_input() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut document_input = ports::MockBootstrapSecretDocumentInputPort::new();
        document_input
            .expect_read_bootstrap_secret_fields()
            .times(0);
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .returning(|_, _| {
                Ok(SecretStorageSetupInspection {
                    key_exists: true,
                    piv_version: PivApplicationVersion::minimum_for_secret_storage(),
                    manifest_bytes: Some(
                        crate::domain::manifest::SecretManifest::fixture_v2().encode()?,
                    ),
                    occupied_object_ids: Vec::new(),
                })
            });
        storage.expect_initialize_secret_storage().times(0);
        storage.expect_store_secret().times(0);
        let report = ports::MockReportPort::new();

        let result = run_enroll_primary_with_stdin_json(
            EnrollPrimaryCommand { serial: Some(2001) },
            &mut device_serial,
            &document_input,
            &mut storage,
            &report,
        );

        assert!(
            result.is_err(),
            "existing storage must stop before document input"
        );
    }

    #[test]
    fn enroll_primary_stdin_json_stops_when_setup_initialization_fails() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Ok(2001));
        let mut document_input = ports::MockBootstrapSecretDocumentInputPort::new();
        document_input
            .expect_read_bootstrap_secret_fields()
            .times(0)
            .returning(|| Ok(fields()));
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

        let result = run_enroll_primary_with_stdin_json(
            EnrollPrimaryCommand { serial: Some(2001) },
            &mut device_serial,
            &document_input,
            &mut storage,
            &report,
        );

        assert!(result.is_err(), "setup failure must stop before store");
    }
}
