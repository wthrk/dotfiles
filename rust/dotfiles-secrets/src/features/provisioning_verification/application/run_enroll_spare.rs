//! `enroll-spare` の PIV management lifecycle を bootstrap document input から分離する。

use crate::{
    Result,
    features::provisioning_verification::domain::commands::EnrollSpareCommand,
    features::{
        cli_interaction::ports::public::{BootstrapDocumentInputPort, ReportPort},
        provisioning_verification::domain::enrollment::EnrollSummary,
        yubikey_lifecycle::{
            domain::{
                manifest::{BootstrapSecretDocument, BootstrapSecretDocumentReadPlan},
                piv::SecretName,
                storage::{
                    SecretStorageReadIntent, SecretStorageSetupIntent, SecretStorageSetupProbe,
                    SecretStorageVerificationPlan, SecretStorageWriteIntent,
                    is_secret_storage_ownership_unknown,
                },
            },
            ports::public::piv_pin_input::PivPinInputPort,
            ports::{DeviceSerialPort, SecretStoragePort},
        },
    },
};

/// 選択済み bootstrap document を spare に再暗号化して保存する。
pub(crate) fn run_enroll_spare(
    command: EnrollSpareCommand,
    device: &mut dyn DeviceSerialPort,
    piv_pin: &dyn PivPinInputPort,
    document_input: Option<&mut dyn BootstrapDocumentInputPort>,
    storage: &mut dyn SecretStoragePort,
    report: &dyn ReportPort,
) -> Result<()> {
    command.ensure_requested_serials_distinct()?;
    let spare_serial = device.resolve_device_serial(command.spare_serial)?;
    device
        .inspect_device_profile(spare_serial)?
        .ensure_pin_free_recovery_supported()?;
    command.ensure_requested_primary_differs_from_spare(spare_serial)?;
    let primary_serial = if document_input.is_none() {
        let primary_serial = device.resolve_device_serial(command.primary_serial)?;
        device
            .inspect_device_profile(primary_serial)?
            .ensure_pin_free_recovery_supported()?;
        command.ensure_distinct_resolved_serials(primary_serial, spare_serial)?;
        Some(primary_serial)
    } else {
        None
    };
    // current PIN は spare の完全管理 inspection を開始する前に一回だけ取得する。fresh と domain が
    // 確定するまで new/confirmation を読まず、initialized spare はこの management session を継続する。
    let current_pin = piv_pin.read_current_piv_pin_secret()?;
    storage
        .begin_piv_pin_setup_preflight(spare_serial, &current_pin)
        .map_err(opaque_enrollment_failure)?;

    let setup_probe = SecretStorageSetupProbe::expected();
    let setup_inspection = storage
        .inspect_secret_storage_setup(spare_serial, &setup_probe)
        .map_err(opaque_enrollment_failure)?;
    let setup_intent = SecretStorageSetupIntent::for_enrollment(setup_inspection)
        .map_err(opaque_enrollment_failure)?;
    let initialized_write_intent = if setup_intent.requires_initialized_write_preflight() {
        let storage_spec = SecretName::BitwardenClientSecret.storage_spec(spare_serial);
        let inspection = storage
            .inspect_secret_storage_write(spare_serial, &storage_spec)
            .map_err(opaque_enrollment_failure)?;
        Some(
            SecretStorageWriteIntent::preflight_initial_enrollment(storage_spec, &inspection)
                .map_err(opaque_enrollment_failure)?,
        )
    } else {
        None
    };
    let document_input = match document_input {
        Some(document_input) => document_input
            .read_bootstrap_secret_document_input()
            .map_err(opaque_enrollment_failure)?,
        None => {
            let primary_serial = primary_serial.ok_or_else(|| {
                anyhow::anyhow!(
                    "primary YubiKey serial was not resolved for bootstrap document read"
                )
            })?;
            read_primary_bootstrap_document(primary_serial, storage)
                .map_err(opaque_enrollment_failure)?
        }
    };
    let document =
        BootstrapSecretDocument::from_input(document_input).map_err(opaque_enrollment_failure)?;
    // Fresh spare は primary/supplied bootstrap document の read・decrypt・parse・全 domain validation
    // を終えてからだけ application-wide PIN を変更する。変更後は新 PIN 認証と保存を連続させる。
    if setup_intent.requires_piv_pin_change() {
        let new_pin = piv_pin
            .read_new_piv_pin_confirmation()
            .map_err(opaque_enrollment_failure)?;
        storage
            .change_piv_pin(spare_serial, &current_pin, &new_pin)
            .map_err(opaque_enrollment_failure)?;
        storage
            .begin_piv_management_session(spare_serial, new_pin)
            .map_err(opaque_enrollment_failure)?;
    }
    let public_key_spki = storage
        .initialize_secret_storage(spare_serial, setup_intent.clone())
        .map_err(opaque_enrollment_failure)?;
    for (storage_spec, value) in document.storage_entries(spare_serial) {
        let intent = if setup_intent.requires_finalization() {
            SecretStorageWriteIntent::initial_enroll_store(
                storage_spec,
                value.len(),
                public_key_spki.clone(),
            )?
        } else {
            initialized_write_intent
                .clone()
                .ok_or_else(|| anyhow::anyhow!("initialized enrollment preflight is missing"))?
                .with_initial_enrollment_secret_len(value.len())
                .map_err(opaque_enrollment_failure)?
        };
        storage
            .store_secret(spare_serial, intent, value)
            .map_err(opaque_enrollment_failure)?;
    }
    if setup_intent.requires_finalization() {
        storage
            .finalize_secret_storage_setup(
                spare_serial,
                setup_intent
                    .manifest_for_public_key(public_key_spki)
                    .map_err(opaque_enrollment_failure)?,
            )
            .map_err(opaque_enrollment_failure)?;
    }
    verify_local_storage(spare_serial, storage).map_err(opaque_enrollment_failure)?;
    report.write_enroll_report(&EnrollSummary::spare_completed(spare_serial))
}

/// enrollment の card/storage/PIN 由来 failure を固定文言に閉じ、原因 chain は診断境界へ保持する。
fn opaque_enrollment_failure(error: anyhow::Error) -> anyhow::Error {
    if is_secret_storage_ownership_unknown(&error) {
        return error
            .context("YubiKey PIV enrollment failed; manual administrator escalation is required");
    }
    error.context("YubiKey PIV enrollment failed")
}

/// primary の PIN-free storage read を、spare preflight 後に domain read plan へ従って実行する。
///
/// storage backend は PIV I/O だけを担い、storage 対象、manifest/blob の整合、decode failure の意味は
/// domain read plan が固定する。application は primary read を spare の準備前には行わない。
fn read_primary_bootstrap_document(
    primary_serial: u32,
    storage: &mut dyn SecretStoragePort,
) -> Result<crate::features::yubikey_lifecycle::domain::manifest::BootstrapSecretDocumentInput> {
    let plan = BootstrapSecretDocumentReadPlan::for_storage(
        SecretName::BitwardenClientSecret.storage_spec(primary_serial),
    );
    let inspection = storage.inspect_secret_storage_read(primary_serial, plan.storage())?;
    let intent = plan.read_intent(inspection)?;
    let loaded = storage.load_secret(primary_serial, &intent);
    plan.document(&intent, loaded)
}

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
    use crate::{
        features::{
            provisioning_verification::domain::commands::EnrollSpareCommand,
            yubikey_lifecycle::domain::{
                self as domain,
                manifest::SecretManifest,
                piv::{PivApplicationVersion, SecretName},
                storage::{
                    SecretStorageReadInspection, SecretStorageSetupInspection,
                    SecretStorageWriteInspection,
                },
            },
        },
        foundation::protection::ProtectedSecret,
    };
    mod ports {
        pub(crate) use crate::features::cli_interaction::ports::public::{
            MockBootstrapDocumentInputPort, MockReportPort,
        };
        pub(crate) mod io {
            pub(crate) use crate::features::yubikey_lifecycle::ports::public::piv_pin_input::MockPivPinInputPort;
        }
    }
    use mockall::Sequence;

    use super::run_enroll_spare;

    fn expect_pin_free_device_profile(
        device: &mut crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort,
    ) {
        device.expect_inspect_device_profile().returning(|_| {
            Ok(domain::piv::PivDeviceProfile {
                version: PivApplicationVersion {
                    major: 5,
                    minor: 7,
                    patch: 1,
                },
                fips_series: false,
            })
        });
    }

    fn material(bytes: &'static [u8]) -> crate::Result<ProtectedSecret> {
        ProtectedSecret::from_test_bytes(bytes)
    }

    fn document() -> crate::Result<domain::manifest::BootstrapSecretDocumentInput> {
        Ok(
            domain::manifest::BootstrapSecretDocumentInput::BitwardenClientSecret(material(
                b"token",
            )?),
        )
    }

    #[test]
    fn spare_runner_uses_one_session_for_a_supplied_document() -> crate::Result<()> {
        let mut device =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        expect_pin_free_device_profile(&mut device);
        let mut sequence = Sequence::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2002));
        let mut pin = ports::io::MockPivPinInputPort::new();
        pin.expect_read_current_piv_pin_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| material(b"123456"));
        let mut document_input = ports::MockBootstrapDocumentInputPort::new();
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_pin_setup_preflight()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|serial, _| *serial == 2002)
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
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
        document_input
            .expect_read_bootstrap_secret_document_input()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(document);
        pin.expect_read_new_piv_pin_confirmation()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| material(b"654321"));
        storage
            .expect_change_piv_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|serial, current, new| {
                *serial == 2002
                    && current.to_test_bytes() == b"123456"
                    && new.to_test_bytes() == b"654321"
            })
            .returning(|_, _, _| Ok(()));
        storage
            .expect_begin_piv_management_session()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|serial, pin| *serial == 2002 && pin.to_test_bytes() == b"654321")
            .returning(|_, _| Ok(()));
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| {
                SecretManifest::fixture_v2()
                    .slot_public_key_spki
                    .ok_or_else(|| anyhow::anyhow!("fixture v2 manifest must contain SPKI"))
            });
        storage
            .expect_store_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _, _| Ok(()));
        storage
            .expect_finalize_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|serial, spec| *serial == 2002 && spec.name == SecretName::BitwardenClientSecret)
            .returning(|_, _| {
                Ok(SecretStorageReadInspection {
                    manifest_bytes: Some(SecretManifest::fixture_v2().encode()?),
                    encoded: Some(vec![1]),
                })
            });
        storage
            .expect_load_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| material(b"token"));
        let mut report = ports::MockReportPort::new();
        report
            .expect_write_enroll_report()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(()));

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

    #[test]
    fn fresh_spare_pin_change_failure_occurs_after_document_validation() -> crate::Result<()> {
        let mut device =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        expect_pin_free_device_profile(&mut device);
        let mut pin = ports::io::MockPivPinInputPort::new();
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        let mut document_input = ports::MockBootstrapDocumentInputPort::new();
        let report = ports::MockReportPort::new();
        let mut sequence = Sequence::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2002));
        pin.expect_read_current_piv_pin_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| material(b"123456"));
        storage
            .expect_begin_piv_pin_setup_preflight()
            .withf(|serial, current| *serial == 2002 && current.to_test_bytes() == b"123456")
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
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
        document_input
            .expect_read_bootstrap_secret_document_input()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(document);
        pin.expect_read_new_piv_pin_confirmation()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| material(b"654321"));
        storage
            .expect_change_piv_pin()
            .withf(|serial, current, new| {
                *serial == 2002
                    && current.to_test_bytes() == b"123456"
                    && new.to_test_bytes() == b"654321"
            })
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _, _| Err(anyhow::anyhow!("PIV change failed")));

        let error = run_enroll_spare(
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
        .err()
        .ok_or_else(|| anyhow::anyhow!("PIN change failure must stop enrollment"))?;
        assert_eq!(error.to_string(), "YubiKey PIV enrollment failed");
        assert!(
            error
                .chain()
                .any(|source| source.to_string() == "PIV change failed"),
            "opaque enrollment error must retain its causal source"
        );
        Ok(())
    }

    #[test]
    fn key_only_spare_escalates_before_document_or_storage_mutation() -> crate::Result<()> {
        let mut device =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        expect_pin_free_device_profile(&mut device);
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Ok(2002));
        let mut pin = ports::io::MockPivPinInputPort::new();
        pin.expect_read_current_piv_pin_secret()
            .times(1)
            .returning(|| material(b"123456"));
        pin.expect_read_new_piv_pin_confirmation().never();
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_pin_setup_preflight()
            .times(1)
            .returning(|_, _| Ok(()));
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
        storage.expect_change_piv_pin().never();
        storage.expect_initialize_secret_storage().never();
        storage.expect_store_secret().never();
        storage.expect_finalize_secret_storage_setup().never();
        let mut document_input = ports::MockBootstrapDocumentInputPort::new();
        document_input
            .expect_read_bootstrap_secret_document_input()
            .never();
        let mut report = ports::MockReportPort::new();
        report.expect_write_enroll_report().never();

        let error = run_enroll_spare(
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
        .expect_err("key-only ownership must require manual escalation");
        assert!(
            error
                .to_string()
                .contains("manual administrator escalation")
        );
        Ok(())
    }

    fn assert_spare_rejects_before_primary_read(
        inspection: SecretStorageSetupInspection,
        initialized_nonempty: bool,
    ) -> crate::Result<()> {
        let mut device =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        expect_pin_free_device_profile(&mut device);
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2002));
        let mut pin = ports::io::MockPivPinInputPort::new();
        pin.expect_read_current_piv_pin_secret()
            .returning(|| material(b"123456"));
        pin.expect_read_new_piv_pin_confirmation().never();
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_pin_setup_preflight()
            .returning(|_, _| Ok(()));
        let mut inspection = Some(inspection);
        storage
            .expect_inspect_secret_storage_setup()
            .returning(move |_, _| {
                inspection
                    .take()
                    .ok_or_else(|| anyhow::anyhow!("setup inspection requested more than once"))
            });
        if initialized_nonempty {
            storage
                .expect_inspect_secret_storage_write()
                .times(1)
                .returning(|_, _| {
                    Ok(SecretStorageWriteInspection {
                        manifest_present: true,
                        manifest_bytes: Some(SecretManifest::fixture_v2().encode()?),
                        object_present: true,
                        object_exists: true,
                        reserved_slot_key_exists: true,
                        reserved_slot_certificate_exists: false,
                        slot_public_key_spki: SecretManifest::fixture_v2().slot_public_key_spki,
                    })
                });
        } else {
            storage.expect_inspect_secret_storage_write().never();
        }
        storage.expect_inspect_secret_storage_read().never();
        storage.expect_initialize_secret_storage().never();
        storage.expect_store_secret().never();
        let mut document = ports::MockBootstrapDocumentInputPort::new();
        document
            .expect_read_bootstrap_secret_document_input()
            .never();
        let mut report = ports::MockReportPort::new();
        report.expect_write_enroll_report().never();

        assert!(
            run_enroll_spare(
                EnrollSpareCommand {
                    primary_serial: Some(2001),
                    spare_serial: Some(2002),
                },
                &mut device,
                &pin,
                Some(&mut document),
                &mut storage,
                &report,
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn spare_rejects_nonempty_and_v1_storage_before_primary_read() -> crate::Result<()> {
        assert_spare_rejects_before_primary_read(
            SecretStorageSetupInspection {
                reserved_slot_key_exists: true,
                reserved_slot_certificate_exists: false,
                slot_public_key_spki: SecretManifest::fixture_v2().slot_public_key_spki,
                piv_version: PivApplicationVersion::minimum_for_secret_storage(),
                manifest_bytes: Some(SecretManifest::fixture_v2().encode()?),
                present_object_ids: vec![
                    domain::piv::PivObjectId::MANIFEST,
                    SecretName::BitwardenClientSecret.object_id(),
                ],
                nonempty_object_ids: vec![
                    domain::piv::PivObjectId::MANIFEST,
                    SecretName::BitwardenClientSecret.object_id(),
                ],
            },
            true,
        )?;
        let v1 = SecretManifest {
            version: 1,
            app: domain::manifest::MANIFEST_APP.to_owned(),
            slot_public_key_spki: None,
        }
        .encode()?;
        assert_spare_rejects_before_primary_read(
            SecretStorageSetupInspection {
                reserved_slot_key_exists: true,
                reserved_slot_certificate_exists: false,
                slot_public_key_spki: SecretManifest::fixture_v2().slot_public_key_spki,
                piv_version: PivApplicationVersion::minimum_for_secret_storage(),
                manifest_bytes: Some(v1),
                present_object_ids: vec![domain::piv::PivObjectId::MANIFEST],
                nonempty_object_ids: vec![domain::piv::PivObjectId::MANIFEST],
            },
            false,
        )
    }
}
