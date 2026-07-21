//! enroll-spare(prompt) の順序を固定し、primary/spare 判定責務を I/O 実装から分離する。

use crate::Result;
use crate::{
    domain::{
        commands::EnrollSpareCommand,
        enrollment::EnrollSummary,
        manifest::BootstrapSecretDocument,
        storage::{
            SecretStorageReadIntent, SecretStorageSetupIntent, SecretStorageSetupProbe,
            SecretStorageVerificationPlan, SecretStorageWriteIntent,
        },
    },
    ports,
};

/// primary YubiKey から読み出した secret を prompt 運用の spare YubiKey へ複製する。
///
/// primary / spare の serial 解決と spare storage の事前検査を primary 読み出しより先に固定する。
/// serial を明示する通常経路では、両 device がこの事前検査と後続の読み書きに利用可能である。
/// secret 転送手段の詳細は port 境界で読み出しと保存を接続する。
pub(crate) fn run_enroll_spare_with_prompt<D, S, R>(
    command: EnrollSpareCommand,
    device: &mut D,
    storage_port: &mut S,
    report: &R,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    S: ports::SecretStoragePort,
    R: ports::ReportPort,
{
    command.ensure_requested_serials_distinct()?;
    let primary_serial = device.resolve_device_serial(command.primary_serial)?;
    let spare_serial = device.resolve_device_serial(command.spare_serial)?;
    command.ensure_distinct_resolved_serials(primary_serial, spare_serial)?;
    let setup_probe = SecretStorageSetupProbe::expected();
    let setup_inspection = storage_port.inspect_secret_storage_setup(spare_serial, &setup_probe)?;
    let setup_intent = SecretStorageSetupIntent::for_enrollment(setup_inspection)?;
    let [first_storage, second_storage, third_storage] =
        crate::domain::piv::SecretStorageSpec::all_for_serial(primary_serial);
    let first_inspection =
        storage_port.inspect_secret_storage_read(primary_serial, &first_storage)?;
    let first_intent = SecretStorageReadIntent::from_inspection(first_storage, first_inspection)?;
    let first_document_storage = first_intent.storage.clone();
    let first = storage_port
        .load_secret(primary_serial, &first_intent)
        .map_err(|error| first_intent.decode_error(error))?;
    first_intent.validate_loaded_secret(&first)?;
    let second_inspection =
        storage_port.inspect_secret_storage_read(primary_serial, &second_storage)?;
    let second_intent =
        SecretStorageReadIntent::from_inspection(second_storage, second_inspection)?;
    let second_document_storage = second_intent.storage.clone();
    let second = storage_port
        .load_secret(primary_serial, &second_intent)
        .map_err(|error| second_intent.decode_error(error))?;
    second_intent.validate_loaded_secret(&second)?;
    let third_inspection =
        storage_port.inspect_secret_storage_read(primary_serial, &third_storage)?;
    let third_intent = SecretStorageReadIntent::from_inspection(third_storage, third_inspection)?;
    let third_document_storage = third_intent.storage.clone();
    let third = storage_port
        .load_secret(primary_serial, &third_intent)
        .map_err(|error| third_intent.decode_error(error))?;
    third_intent.validate_loaded_secret(&third)?;
    let document = BootstrapSecretDocument::from_storage_materials([
        (first_document_storage, first),
        (second_document_storage, second),
        (third_document_storage, third),
    ])?;
    let public_key_spki =
        storage_port.initialize_secret_storage(spare_serial, setup_intent.clone())?;
    for (storage, value) in document.storage_entries(spare_serial) {
        let intent = SecretStorageWriteIntent::initial_enroll_store(
            storage,
            value.len(),
            public_key_spki.clone(),
        )?;
        storage_port.store_secret(spare_serial, intent, value)?;
    }
    storage_port.finalize_secret_storage_setup(
        spare_serial,
        setup_intent.manifest_for_public_key(public_key_spki)?,
    )?;
    for storage in SecretStorageVerificationPlan::for_serial(spare_serial).into_targets() {
        let inspection = storage_port.inspect_secret_storage_read(spare_serial, &storage)?;
        let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
        let secret = storage_port
            .load_secret(spare_serial, &intent)
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
            manifest_bytes: None,
            occupied_object_ids: Vec::new(),
        }
    }

    fn read_inspection() -> SecretStorageReadInspection {
        SecretStorageReadInspection {
            manifest_bytes: Some(SecretManifest::fixture_v2().encode().expect("manifest")),
            encoded: Some(vec![1]),
        }
    }

    #[test]
    fn enroll_spare_prompt_rejects_same_requested_serials_before_ports() {
        let mut primary_device = ports::MockDeviceSerialPort::new();
        primary_device.expect_resolve_device_serial().times(0);
        let mut storage = ports::MockSecretStoragePort::new();
        storage.expect_inspect_secret_storage_read().times(0);
        let report = ports::MockReportPort::new();

        let result = run_enroll_spare_with_prompt(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2001),
            },
            &mut primary_device,
            &mut storage,
            &report,
        );

        assert!(
            result.is_err(),
            "same requested serials must stop before ports"
        );
    }

    #[test]
    fn enroll_spare_prompt_rejects_existing_spare_before_primary_read() {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|requested| Ok(requested.expect("explicit test serial")));
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|requested| Ok(requested.expect("explicit test serial")));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .withf(|serial, _| *serial == 2002)
            .returning(|_, _| {
                Ok(SecretStorageSetupInspection {
                    key_exists: true,
                    piv_version: PivApplicationVersion::minimum_for_secret_storage(),
                    manifest_bytes: Some(SecretManifest::fixture_v2().encode()?),
                    occupied_object_ids: Vec::new(),
                })
            });
        storage.expect_inspect_secret_storage_read().times(0);
        storage.expect_load_secret().times(0);
        storage.expect_initialize_secret_storage().times(0);
        storage.expect_store_secret().times(0);
        let report = ports::MockReportPort::new();

        let result = run_enroll_spare_with_prompt(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2002),
            },
            &mut device,
            &mut storage,
            &report,
        );

        assert!(
            result.is_err(),
            "existing spare must stop before primary read"
        );
    }

    #[test]
    fn enroll_spare_prompt_preflights_spare_before_reading_primary() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut primary_device = ports::MockDeviceSerialPort::new();
        primary_device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        primary_device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2002));
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(setup_inspection()));
        for name in [
            SecretName::BwEmail,
            SecretName::BwPassword,
            SecretName::BitwardenClientSecret,
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
                .returning(|_, intent| {
                    Ok(match intent.storage.name {
                        SecretName::BwEmail => material(b"email"),
                        SecretName::BwPassword => material(b"password"),
                        SecretName::BitwardenClientSecret => material(b"token"),
                    })
                });
        }
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
            .times(3)
            .returning(|_, _, _| Ok(()));
        storage
            .expect_finalize_secret_storage_setup()
            .times(1)
            .returning(|_, _| Ok(()));
        // The primary supplies all stored bootstrap values for cloning.  The
        // spare's post-write recovery preflight checks only the BWS credential.
        for name in [SecretName::BitwardenClientSecret] {
            storage
                .expect_inspect_secret_storage_read()
                .times(1)
                .withf(move |serial, storage| *serial == 2002 && storage.name == name)
                .returning(|_, _| Ok(read_inspection()));
            storage
                .expect_load_secret()
                .times(1)
                .returning(|_, _| Ok(material(b"token")));
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
        let mut storage = ports::MockSecretStoragePort::new();
        for name in [
            SecretName::BwEmail,
            SecretName::BwPassword,
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
                .returning(|_, intent| {
                    Ok(match intent.storage.name {
                        SecretName::BwEmail => material(b"email"),
                        SecretName::BwPassword => material(b"password"),
                        SecretName::BitwardenClientSecret => material(b"token"),
                    })
                });
        }
        primary_device
            .expect_resolve_device_serial()
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
            &mut storage,
            &report,
        );

        assert!(
            result.is_err(),
            "spare setup failure must stop before store"
        );
    }
}
