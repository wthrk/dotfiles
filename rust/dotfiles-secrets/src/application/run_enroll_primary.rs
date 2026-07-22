//! `enroll-primary` の PIV management lifecycle を document source から分離する。

use crate::{
    Result,
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

/// primary YubiKey の storage を初期化、保存、local verification する。
///
/// `document_input` は entrypoint が選んだ未検証 input carrier の取得だけを担う。
/// carrier の document schema 判定はこの use case が domain constructor を通して行う。
pub(crate) fn run_enroll_primary(
    command: EnrollPrimaryCommand,
    device: &mut dyn ports::DeviceSerialPort,
    piv_pin: &dyn ports::PivPinInputPort,
    document_input: &mut dyn ports::BootstrapDocumentInputPort,
    storage: &mut dyn ports::SecretStoragePort,
    report: &dyn ports::ReportPort,
) -> Result<()> {
    let serial = device.resolve_device_serial(command.serial)?;
    let pin = piv_pin.read_piv_pin_secret()?;
    storage.begin_piv_management_session(serial, pin)?;

    let setup_probe = SecretStorageSetupProbe::expected();
    let setup_inspection = storage.inspect_secret_storage_setup(serial, &setup_probe)?;
    let setup_intent = SecretStorageSetupIntent::for_enrollment(setup_inspection)?;
    // 入力 schema は application/domain の検証であり storage mutation ではない。PIV PIN session は
    // すでに開始済みなので stdin が controlling TTY 境界より先に消費されることはないが、不正な JSON は
    // key generation、object write、finalize の前に停止しなければならない。
    let document = BootstrapSecretDocument::from_input(
        document_input.read_bootstrap_secret_document_input()?,
    )?;
    let public_key_spki = storage.initialize_secret_storage(serial, setup_intent.clone())?;
    for (storage_spec, value) in document.storage_entries(serial) {
        let intent = SecretStorageWriteIntent::initial_enroll_store(
            storage_spec,
            value.len(),
            public_key_spki.clone(),
        )?;
        storage.store_secret(serial, intent, value)?;
    }
    storage.finalize_secret_storage_setup(
        serial,
        setup_intent.manifest_for_public_key(public_key_spki)?,
    )?;
    verify_local_storage(serial, storage)?;
    report.write_enroll_report(&EnrollSummary::primary_completed(serial))
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
            commands::EnrollPrimaryCommand,
            manifest::SecretManifest,
            piv::{PivApplicationVersion, SecretName},
            storage::{SecretStorageReadInspection, SecretStorageSetupInspection},
            verification::{CheckName, CheckStatus},
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_enroll_primary;

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn document_input() -> crate::Result<crate::domain::manifest::BootstrapSecretDocumentInput> {
        Ok(
            crate::domain::manifest::BootstrapSecretDocumentInput::BitwardenClientSecret(material(
                b"token",
            )),
        )
    }

    #[test]
    fn primary_runner_uses_one_session_for_a_normalized_document() -> crate::Result<()> {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut pin = ports::io::MockPivPinInputPort::new();
        pin.expect_read_piv_pin_secret()
            .returning(|| Ok(material(b"123456")));
        let mut document_input_port = ports::MockBootstrapDocumentInputPort::new();
        document_input_port
            .expect_read_bootstrap_secret_document_input()
            .returning(document_input);
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_management_session()
            .times(1)
            .withf(|serial, _| *serial == 2001)
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
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
            .times(1)
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
            .times(1)
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .withf(|serial, spec| *serial == 2001 && spec.name == SecretName::BitwardenClientSecret)
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
        report
            .expect_write_enroll_report()
            .times(1)
            .withf(|summary| {
                summary.serial == 2001
                    && summary.checks.get(&CheckName::LocalStorage) == Some(&CheckStatus::Ok)
            })
            .returning(|_| Ok(()));

        run_enroll_primary(
            EnrollPrimaryCommand { serial: None },
            &mut device,
            &pin,
            &mut document_input_port,
            &mut storage,
            &report,
        )
    }
}
