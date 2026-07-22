//! `enroll-spare` の PIV management lifecycle を bootstrap document input から分離する。

use crate::{
    Result,
    domain::{
        commands::EnrollSpareCommand,
        enrollment::EnrollSummary,
        manifest::{BootstrapSecretDocument, BootstrapSecretDocumentReadPlan},
        piv::SecretName,
        storage::{
            SecretStorageReadIntent, SecretStorageSetupIntent, SecretStorageSetupProbe,
            SecretStorageVerificationPlan, SecretStorageWriteIntent,
        },
    },
    ports,
};

/// 選択済み bootstrap document を spare に再暗号化して保存する。
pub(crate) fn run_enroll_spare(
    command: EnrollSpareCommand,
    device: &mut dyn ports::DeviceSerialPort,
    piv_pin: &dyn ports::PivPinInputPort,
    document_input: Option<&mut dyn ports::BootstrapDocumentInputPort>,
    storage: &mut dyn ports::SecretStoragePort,
    report: &dyn ports::ReportPort,
) -> Result<()> {
    command.ensure_requested_serials_distinct()?;
    let spare_serial = device.resolve_device_serial(command.spare_serial)?;
    command.ensure_requested_primary_differs_from_spare(spare_serial)?;
    let primary_serial = if document_input.is_none() {
        let primary_serial = device.resolve_device_serial(command.primary_serial)?;
        command.ensure_distinct_resolved_serials(primary_serial, spare_serial)?;
        Some(primary_serial)
    } else {
        None
    };
    let pin = piv_pin.read_piv_pin_secret()?;
    storage.begin_piv_management_session(spare_serial, pin)?;

    let setup_probe = SecretStorageSetupProbe::expected();
    let setup_inspection = storage.inspect_secret_storage_setup(spare_serial, &setup_probe)?;
    let setup_intent = SecretStorageSetupIntent::for_enrollment(setup_inspection)?;
    let document_input = match document_input {
        Some(document_input) => document_input.read_bootstrap_secret_document_input()?,
        None => {
            let primary_serial = primary_serial.ok_or_else(|| {
                anyhow::anyhow!(
                    "primary YubiKey serial was not resolved for bootstrap document read"
                )
            })?;
            read_primary_bootstrap_document(primary_serial, storage)?
        }
    };
    let document = BootstrapSecretDocument::from_input(document_input)?;
    let public_key_spki = storage.initialize_secret_storage(spare_serial, setup_intent.clone())?;
    for (storage_spec, value) in document.storage_entries(spare_serial) {
        let intent = SecretStorageWriteIntent::initial_enroll_store(
            storage_spec,
            value.len(),
            public_key_spki.clone(),
        )?;
        storage.store_secret(spare_serial, intent, value)?;
    }
    storage.finalize_secret_storage_setup(
        spare_serial,
        setup_intent.manifest_for_public_key(public_key_spki)?,
    )?;
    verify_local_storage(spare_serial, storage)?;
    report.write_enroll_report(&EnrollSummary::spare_completed(spare_serial))
}

/// primary の PIN-free storage read を、spare preflight 後に domain read plan へ従って実行する。
///
/// storage backend は PIV I/O だけを担い、storage 対象、manifest/blob の整合、decode failure の意味は
/// domain read plan が固定する。application は primary read を spare の準備前には行わない。
fn read_primary_bootstrap_document(
    primary_serial: u32,
    storage: &mut dyn ports::SecretStoragePort,
) -> Result<crate::domain::manifest::BootstrapSecretDocumentInput> {
    let plan = BootstrapSecretDocumentReadPlan::for_storage(
        SecretName::BitwardenClientSecret.storage_spec(primary_serial),
    );
    let inspection = storage.inspect_secret_storage_read(primary_serial, plan.storage())?;
    let intent = plan.read_intent(inspection)?;
    let loaded = storage.load_secret(primary_serial, &intent);
    plan.document(&intent, loaded)
}

fn verify_local_storage(serial: u32, storage: &mut dyn ports::SecretStoragePort) -> Result<()> {
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

    use super::run_enroll_spare;

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn document() -> crate::Result<crate::domain::manifest::BootstrapSecretDocumentInput> {
        Ok(
            crate::domain::manifest::BootstrapSecretDocumentInput::BitwardenClientSecret(material(
                b"token",
            )),
        )
    }

    #[test]
    fn spare_runner_uses_one_session_for_a_supplied_document() -> crate::Result<()> {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2002));
        let mut pin = ports::io::MockPivPinInputPort::new();
        pin.expect_read_piv_pin_secret()
            .returning(|| Ok(material(b"123456")));
        let mut document_input = ports::MockBootstrapDocumentInputPort::new();
        document_input
            .expect_read_bootstrap_secret_document_input()
            .returning(document);
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_management_session()
            .times(1)
            .withf(|serial, _| *serial == 2002)
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_setup()
            .returning(|_, _| {
                Ok(SecretStorageSetupInspection {
                    key_exists: false,
                    piv_version: PivApplicationVersion::minimum_for_secret_storage(),
                    manifest_bytes: None,
                    occupied_object_ids: Vec::new(),
                })
            });
        storage
            .expect_initialize_secret_storage()
            .returning(|_, _| {
                Ok(SecretManifest::fixture_v2()
                    .slot_public_key_spki
                    .expect("fixture SPKI"))
            });
        storage
            .expect_store_secret()
            .times(1)
            .returning(|_, _, _| Ok(()));
        storage
            .expect_finalize_secret_storage_setup()
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .withf(|serial, spec| *serial == 2002 && spec.name == SecretName::BitwardenClientSecret)
            .returning(|_, _| {
                Ok(SecretStorageReadInspection {
                    manifest_bytes: Some(SecretManifest::fixture_v2().encode().expect("manifest")),
                    encoded: Some(vec![1]),
                })
            });
        storage
            .expect_load_secret()
            .times(1)
            .returning(|_, _| Ok(material(b"token")));
        let mut report = ports::MockReportPort::new();
        report.expect_write_enroll_report().returning(|_| Ok(()));

        run_enroll_spare(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2002),
            },
            &mut device,
            &pin,
            Some(&mut document_input),
            &mut storage,
            &report,
        )
    }
}
