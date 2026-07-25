//! `enroll-primary` の PIV management lifecycle を document source から分離する。

use crate::{
    Result,
    features::provisioning_verification::domain::commands::EnrollPrimaryCommand,
    features::{
        cli_interaction::ports::public::{BootstrapDocumentInputPort, ReportPort},
        provisioning_verification::domain::enrollment::EnrollSummary,
        yubikey_lifecycle::{
            domain::{
                manifest::BootstrapSecretDocument,
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

/// primary YubiKey の storage を初期化、保存、local verification する。
///
/// `document_input` は entrypoint が選んだ未検証 input carrier の取得だけを担う。
/// carrier の document schema 判定はこの use case が domain constructor を通して行う。
pub(crate) fn run_enroll_primary(
    command: EnrollPrimaryCommand,
    device: &mut dyn DeviceSerialPort,
    piv_pin: &dyn PivPinInputPort,
    document_input: &mut dyn BootstrapDocumentInputPort,
    storage: &mut dyn SecretStoragePort,
    report: &dyn ReportPort,
) -> Result<()> {
    let serial = device.resolve_device_serial(command.serial)?;
    device
        .inspect_device_profile(serial)?
        .ensure_pin_free_recovery_supported()?;
    // current PIN は enrollment の最初に一回だけ取得し、同じ handle の VERIFY と protected
    // management-key authentication を通して storage の完全 inspection を許可する。initialized
    // storage はこの session のまま進め、fresh と確定した場合だけ後段で new/confirmation を読む。
    let current_pin = piv_pin.read_current_piv_pin_secret()?;
    storage
        .begin_piv_pin_setup_preflight(serial, &current_pin)
        .map_err(opaque_enrollment_failure)?;

    let setup_probe = SecretStorageSetupProbe::expected();
    let setup_inspection = storage
        .inspect_secret_storage_setup(serial, &setup_probe)
        .map_err(opaque_enrollment_failure)?;
    let setup_intent = SecretStorageSetupIntent::for_enrollment(setup_inspection)
        .map_err(opaque_enrollment_failure)?;
    let initialized_write_intent = if setup_intent.requires_initialized_write_preflight() {
        let storage_spec = SecretName::BitwardenClientSecret.storage_spec(serial);
        let inspection = storage
            .inspect_secret_storage_write(serial, &storage_spec)
            .map_err(opaque_enrollment_failure)?;
        Some(
            SecretStorageWriteIntent::preflight_initial_enrollment(storage_spec, &inspection)
                .map_err(opaque_enrollment_failure)?,
        )
    } else {
        None
    };
    // 入力 schema は application/domain の検証であり storage mutation ではない。PIV PIN session は
    // すでに開始済みなので stdin が controlling TTY 境界より先に消費されることはないが、不正な JSON は
    // key generation、object write、finalize の前に停止しなければならない。
    let document = BootstrapSecretDocument::from_input(
        document_input
            .read_bootstrap_secret_document_input()
            .map_err(opaque_enrollment_failure)?,
    )
    .map_err(opaque_enrollment_failure)?;
    // Fresh enrollment は bootstrap input の取得・decode・domain validation をすべて完了してから
    // application-wide PIN を変更する。変更後は新 PIN 認証から initialize/store/finalize までを
    // 連続させ、変更可否を再判定する preflight を置かない。
    if setup_intent.requires_piv_pin_change() {
        let new_pin = piv_pin
            .read_new_piv_pin_confirmation()
            .map_err(opaque_enrollment_failure)?;
        storage
            .change_piv_pin(serial, &current_pin, &new_pin)
            .map_err(opaque_enrollment_failure)?;
        storage
            .begin_piv_management_session(serial, new_pin)
            .map_err(opaque_enrollment_failure)?;
    }
    let public_key_spki = storage
        .initialize_secret_storage(serial, setup_intent.clone())
        .map_err(opaque_enrollment_failure)?;
    for (storage_spec, value) in document.storage_entries(serial) {
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
            .store_secret(serial, intent, value)
            .map_err(opaque_enrollment_failure)?;
    }
    if setup_intent.requires_finalization() {
        storage
            .finalize_secret_storage_setup(
                serial,
                setup_intent
                    .manifest_for_public_key(public_key_spki)
                    .map_err(opaque_enrollment_failure)?,
            )
            .map_err(opaque_enrollment_failure)?;
    }
    verify_local_storage(serial, storage).map_err(opaque_enrollment_failure)?;
    report.write_enroll_report(&EnrollSummary::primary_completed(serial))
}

/// enrollment の card/storage/PIN 由来 failure を固定文言に閉じ、原因 chain は診断境界へ保持する。
fn opaque_enrollment_failure(error: anyhow::Error) -> anyhow::Error {
    if is_secret_storage_ownership_unknown(&error) {
        return error
            .context("YubiKey PIV enrollment failed; manual administrator escalation is required");
    }
    error.context("YubiKey PIV enrollment failed")
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
            provisioning_verification::domain::{
                commands::EnrollPrimaryCommand,
                verification::{CheckName, CheckStatus},
            },
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

    use super::run_enroll_primary;

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

    fn document_input() -> crate::Result<domain::manifest::BootstrapSecretDocumentInput> {
        Ok(
            domain::manifest::BootstrapSecretDocumentInput::BitwardenClientSecret(material(
                b"token",
            )?),
        )
    }

    #[test]
    fn primary_runner_uses_one_session_for_a_normalized_document() -> crate::Result<()> {
        let mut device =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        expect_pin_free_device_profile(&mut device);
        let mut sequence = Sequence::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        let mut pin = ports::io::MockPivPinInputPort::new();
        pin.expect_read_current_piv_pin_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| material(b"123456"));
        let mut document_input_port = ports::MockBootstrapDocumentInputPort::new();
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_pin_setup_preflight()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|serial, _| *serial == 2001)
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
        document_input_port
            .expect_read_bootstrap_secret_document_input()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(document_input);
        pin.expect_read_new_piv_pin_confirmation()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| material(b"654321"));
        storage
            .expect_change_piv_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|serial, current, new| {
                *serial == 2001
                    && current.to_test_bytes() == b"123456"
                    && new.to_test_bytes() == b"654321"
            })
            .returning(|_, _, _| Ok(()));
        storage
            .expect_begin_piv_management_session()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|serial, pin| *serial == 2001 && pin.to_test_bytes() == b"654321")
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
            .withf(|serial, spec| *serial == 2001 && spec.name == SecretName::BitwardenClientSecret)
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

    #[test]
    fn fresh_primary_pin_change_failure_occurs_after_document_validation() -> crate::Result<()> {
        let mut device =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        expect_pin_free_device_profile(&mut device);
        let mut pin = ports::io::MockPivPinInputPort::new();
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        let mut document_input_port = ports::MockBootstrapDocumentInputPort::new();
        let report = ports::MockReportPort::new();
        let mut sequence = Sequence::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        pin.expect_read_current_piv_pin_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| material(b"123456"));
        storage
            .expect_begin_piv_pin_setup_preflight()
            .withf(|serial, current| *serial == 2001 && current.to_test_bytes() == b"123456")
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
        document_input_port
            .expect_read_bootstrap_secret_document_input()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(document_input);
        pin.expect_read_new_piv_pin_confirmation()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| material(b"654321"));
        storage
            .expect_change_piv_pin()
            .withf(|serial, current, new| {
                *serial == 2001
                    && current.to_test_bytes() == b"123456"
                    && new.to_test_bytes() == b"654321"
            })
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _, _| Err(anyhow::anyhow!("PIV change failed")));

        let error = run_enroll_primary(
            EnrollPrimaryCommand { serial: Some(2001) },
            &mut device,
            &pin,
            &mut document_input_port,
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
    fn key_only_primary_escalates_before_document_or_storage_mutation() -> crate::Result<()> {
        let mut device =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        expect_pin_free_device_profile(&mut device);
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Ok(2001));
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

        let error = run_enroll_primary(
            EnrollPrimaryCommand { serial: Some(2001) },
            &mut device,
            &pin,
            &mut document_input,
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

    fn assert_primary_rejects_before_document(
        inspection: SecretStorageSetupInspection,
        initialized_nonempty: bool,
    ) -> crate::Result<()> {
        let mut device =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        expect_pin_free_device_profile(&mut device);
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
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
        storage.expect_initialize_secret_storage().never();
        storage.expect_store_secret().never();
        let mut document = ports::MockBootstrapDocumentInputPort::new();
        document
            .expect_read_bootstrap_secret_document_input()
            .never();
        let mut report = ports::MockReportPort::new();
        report.expect_write_enroll_report().never();

        assert!(
            run_enroll_primary(
                EnrollPrimaryCommand { serial: Some(2001) },
                &mut device,
                &pin,
                &mut document,
                &mut storage,
                &report,
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn primary_rejects_nonempty_and_v1_storage_before_document_input() -> crate::Result<()> {
        assert_primary_rejects_before_document(
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
        assert_primary_rejects_before_document(
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
