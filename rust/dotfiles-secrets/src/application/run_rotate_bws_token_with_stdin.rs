//! rotate-bws-token(stdin) の順序を固定し、stdin 入力仕様変更を token 更新規則へ混在させない。

use crate::Result;
use crate::{
    domain::{
        commands::RotateBwsTokenCommand,
        storage::{
            SecretStorageReadIntent, SecretStorageVerificationPlan, SecretStorageWriteIntent,
        },
        verification::VerifySummary,
    },
    ports,
};

/// stdin 入力で client-secret を更新し、YubiKey 保存状態を再検証する。
///
/// token 読み取り方式は port 境界で差し替え、use case 側では serial 自動検出と
/// token 入力前の既存 local storage 検証を固定する。
pub(crate) fn run_rotate_bws_token_with_stdin<D, I, S, R>(
    command: RotateBwsTokenCommand,
    device: &mut D,
    secret_input: &I,
    storage_port: &mut S,
    report: &R,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    I: ports::SecretInputPort,
    S: ports::SecretStoragePort,
    R: ports::ReportPort,
{
    let serial = device.resolve_device_serial(command.serial)?;
    let storage = command.storage_spec(serial);
    let inspection = storage_port.inspect_secret_storage_write(serial, &storage)?;
    let _preflight = SecretStorageWriteIntent::preflight_store(storage.clone(), &inspection)?;
    let pre_update_verify: Result<()> = (|| {
        for storage in SecretStorageVerificationPlan::for_serial(serial).into_targets() {
            let inspection = storage_port.inspect_secret_storage_read(serial, &storage)?;
            let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
            let secret = storage_port
                .load_secret(serial, &intent)
                .map_err(|error| intent.decode_error(error))?;
            intent.validate_loaded_secret(&secret)?;
        }
        Ok(())
    })();
    if let Err(err) = pre_update_verify {
        return report
            .write_verify_report(&VerifySummary::local_storage_failed(serial))
            .and(Err(err));
    }
    let token = secret_input.read_streamed_secret()?;
    let intent = SecretStorageWriteIntent::store(storage, inspection, token.len())?;
    storage_port.store_secret(serial, intent, &token)?;
    let verify_result: Result<()> = (|| {
        for storage in SecretStorageVerificationPlan::for_serial(serial).into_targets() {
            let inspection = storage_port.inspect_secret_storage_read(serial, &storage)?;
            let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
            let secret = storage_port
                .load_secret(serial, &intent)
                .map_err(|error| intent.decode_error(error))?;
            intent.validate_loaded_secret(&secret)?;
        }
        Ok(())
    })();
    match verify_result {
        Ok(()) => report.write_verify_report(&VerifySummary::local_storage_verified(serial)),
        Err(err) => report
            .write_verify_report(&VerifySummary::local_storage_failed(serial))
            .and(Err(err)),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{
            commands::RotateBwsTokenCommand,
            manifest::SecretManifest,
            piv::SecretName,
            storage::{SecretStorageReadInspection, SecretStorageWriteInspection},
            verification::{CheckName, CheckStatus},
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_rotate_bws_token_with_stdin;

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn manifest() -> Vec<u8> {
        SecretManifest::fixture_v2().encode().expect("manifest")
    }

    fn write_inspection(object_exists: bool) -> SecretStorageWriteInspection {
        SecretStorageWriteInspection {
            manifest_bytes: Some(manifest()),
            object_present: object_exists,
            object_exists,
            reserved_slot_key_exists: true,
            reserved_slot_certificate_exists: false,
            slot_public_key_spki: Some(
                SecretManifest::fixture_v2()
                    .slot_public_key_spki
                    .expect("SPKI"),
            ),
        }
    }

    fn read_inspection(encoded: bool) -> SecretStorageReadInspection {
        SecretStorageReadInspection {
            manifest_bytes: Some(manifest()),
            encoded: encoded.then_some(vec![1]),
        }
    }

    fn expect_local_verify_ok(
        storage: &mut ports::MockSecretStoragePort,
        sequence: &mut mockall::Sequence,
        serial: u32,
    ) {
        let name = SecretName::BitwardenClientSecret;
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .in_sequence(sequence)
            .withf(move |actual_serial, storage| {
                *actual_serial == serial && storage.name == name
            })
            .returning(|_, _| Ok(read_inspection(true)));
        storage
            .expect_load_secret()
            .times(1)
            .in_sequence(sequence)
            .withf(move |actual_serial, intent| {
                *actual_serial == serial && intent.storage.name == name
            })
            .returning(|_, _| Ok(material(b"client-secret")));
    }

    #[test]
    fn rotate_stdin_auto_detects_serial_from_device_port() {
        let mut serial_port = ports::MockDeviceSerialPort::new();
        serial_port
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Err(anyhow::anyhow!("no device connected")));
        let secret_input = ports::MockSecretInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        storage.expect_inspect_secret_storage_write().times(0);
        let report = ports::MockReportPort::new();

        let result = run_rotate_bws_token_with_stdin(
            RotateBwsTokenCommand { serial: None },
            &mut serial_port,
            &secret_input,
            &mut storage,
            &report,
        );

        assert!(result.is_err(), "device resolution failure should surface");
    }

    #[test]
    fn rotate_stdin_checks_storage_before_reading_token_and_reports() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut serial_port = ports::MockDeviceSerialPort::new();
        serial_port
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Ok(2001));
        let mut secret_input = ports::MockSecretInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|serial, storage| {
                *serial == 2001 && storage.name == SecretName::BitwardenClientSecret
            })
            .returning(|_, _| Ok(write_inspection(false)));
        expect_local_verify_ok(&mut storage, &mut sequence, 2001);
        secret_input
            .expect_read_streamed_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(b"new-token")));
        storage
            .expect_store_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|serial, intent, secret| {
                *serial == 2001
                    && intent.storage.name == SecretName::BitwardenClientSecret
                    && secret.len() == b"new-token".len()
            })
            .returning(|_, _, _| Ok(()));
        expect_local_verify_ok(&mut storage, &mut sequence, 2001);

        let mut report = ports::MockReportPort::new();
        report
            .expect_write_verify_report()
            .times(1)
            .withf(|summary| {
                summary.serial == 2001
                    && summary.checks.get(&CheckName::LocalStorage) == Some(&CheckStatus::Ok)
            })
            .returning(|_| Ok(()));

        run_rotate_bws_token_with_stdin(
            RotateBwsTokenCommand { serial: None },
            &mut serial_port,
            &secret_input,
            &mut storage,
            &report,
        )
    }

    #[test]
    fn rotate_stdin_stops_before_token_read_when_existing_storage_invalid() {
        let mut sequence = mockall::Sequence::new();
        let mut serial_port = ports::MockDeviceSerialPort::new();
        serial_port
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Ok(2001));
        let mut secret_input = ports::MockSecretInputPort::new();
        secret_input.expect_read_streamed_secret().times(0);

        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(write_inspection(false)));
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(read_inspection(false)));
        storage.expect_load_secret().times(0);
        storage.expect_store_secret().times(0);

        let mut report = ports::MockReportPort::new();
        report
            .expect_write_verify_report()
            .times(1)
            .withf(|summary| {
                summary.serial == 2001
                    && summary.checks.get(&CheckName::LocalStorage) == Some(&CheckStatus::Failed)
            })
            .returning(|_| Ok(()));

        let result = run_rotate_bws_token_with_stdin(
            RotateBwsTokenCommand { serial: None },
            &mut serial_port,
            &secret_input,
            &mut storage,
            &report,
        );

        assert!(
            result.is_err(),
            "invalid storage must stop before stdin read"
        );
    }
}
