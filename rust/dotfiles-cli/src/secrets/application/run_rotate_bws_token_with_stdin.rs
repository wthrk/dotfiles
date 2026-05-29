//! rotate-bws-token(stdin) の順序を固定し、stdin 入力仕様変更を token 更新規則へ混在させない。

use crate::Result;
use crate::secrets::{
    domain::{
        command::RotateBwsTokenCommand,
        piv::validate_piv_pin_len,
        storage::{
            SecretStorageReadIntent, SecretStorageVerificationPlan, SecretStorageWriteIntent,
        },
        summary::VerifySummary,
    },
    ports,
};

/// stdin 入力で BWS token を更新し、YubiKey 保存状態を再検証する。
///
/// token 読み取り方式は port 境界で差し替え、use case 側では serial 必須条件と
/// token 入力前の既存 local storage 検証を固定する。
pub(crate) fn run_rotate_bws_token_with_stdin<D, I, P, S, R>(
    command: RotateBwsTokenCommand,
    device: &mut D,
    secret_input: &I,
    pin_input: &P,
    storage_port: &mut S,
    report: &R,
) -> Result<()>
where
    D: ports::yubikey::DevicePinPolicyPort,
    I: ports::io::SecretInputPort,
    P: ports::io::PinInputPort,
    S: ports::yubikey::SecretStoragePort,
    R: ports::io::ReportPort,
{
    let serial = command.required_serial()?;
    let storage = command.storage_spec(serial);
    let inspection = storage_port.inspect_secret_storage_write(serial, &storage)?;
    SecretStorageWriteIntent::ensure_store_preconditions(&inspection)?;
    let pin = if device.device_requires_pin(serial)? {
        let pin = pin_input.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };
    let pre_update_verify: Result<()> = (|| {
        for storage in SecretStorageVerificationPlan::for_serial(serial).into_targets() {
            let inspection = storage_port.inspect_secret_storage_read(serial, &storage)?;
            let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
            let secret = storage_port
                .load_secret(serial, &intent, pin.as_ref())
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
                .load_secret(serial, &intent, pin.as_ref())
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
    use crate::secrets::{
        domain::{
            command::RotateBwsTokenCommand,
            manifest::SecretManifest,
            piv::SecretName,
            storage::{SecretStorageReadInspection, SecretStorageWriteInspection},
            summary::{CheckName, CheckStatus},
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_rotate_bws_token_with_stdin;

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn manifest() -> Vec<u8> {
        SecretManifest::expected().encode().expect("manifest")
    }

    fn write_inspection(object_exists: bool) -> SecretStorageWriteInspection {
        SecretStorageWriteInspection {
            manifest_bytes: Some(manifest()),
            object_exists,
        }
    }

    fn read_inspection(encoded: bool) -> SecretStorageReadInspection {
        SecretStorageReadInspection {
            manifest_bytes: Some(manifest()),
            encoded: encoded.then_some(vec![1]),
        }
    }

    fn expect_local_verify_ok(
        storage: &mut ports::yubikey::MockSecretStoragePort,
        sequence: &mut mockall::Sequence,
        serial: u32,
    ) {
        for name in [
            SecretName::BwEmail,
            SecretName::BwPassword,
            SecretName::BwsAccessToken,
        ] {
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
                .withf(move |actual_serial, intent, _| {
                    *actual_serial == serial && intent.storage.name == name
                })
                .returning(|_, intent, _| {
                    Ok(match intent.storage.name {
                        SecretName::BwEmail => material(b"email"),
                        SecretName::BwPassword => material(b"password"),
                        SecretName::BwsAccessToken => material(b"access-token"),
                    })
                });
        }
    }

    #[test]
    fn rotate_stdin_requires_serial_before_ports() {
        let mut device = ports::yubikey::MockDevicePinPolicyPort::new();
        device.expect_device_requires_pin().times(0);
        let secret_input = ports::io::MockSecretInputPort::new();
        let pin_input = ports::io::MockPinInputPort::new();
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        storage.expect_inspect_secret_storage_write().times(0);
        let report = ports::io::MockReportPort::new();

        let result = run_rotate_bws_token_with_stdin(
            RotateBwsTokenCommand { serial: None },
            &mut device,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
        );

        assert!(result.is_err(), "stdin rotate requires explicit serial");
    }

    #[test]
    fn rotate_stdin_checks_storage_before_reading_token_and_reports() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::yubikey::MockDevicePinPolicyPort::new();
        device
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(false));
        let mut secret_input = ports::io::MockSecretInputPort::new();
        let mut pin_input = ports::io::MockPinInputPort::new();
        pin_input.expect_read_pin().times(0);

        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|serial, storage| *serial == 2001 && storage.name == SecretName::BwsAccessToken)
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
                    && intent.storage.name == SecretName::BwsAccessToken
                    && secret.len() == b"new-token".len()
            })
            .returning(|_, _, _| Ok(()));
        expect_local_verify_ok(&mut storage, &mut sequence, 2001);

        let mut report = ports::io::MockReportPort::new();
        report
            .expect_write_verify_report()
            .times(1)
            .withf(|summary| {
                summary.serial == 2001
                    && summary.checks.get(&CheckName::LocalStorage) == Some(&CheckStatus::Ok)
            })
            .returning(|_| Ok(()));

        run_rotate_bws_token_with_stdin(
            RotateBwsTokenCommand { serial: Some(2001) },
            &mut device,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
        )
    }

    #[test]
    fn rotate_stdin_stops_before_token_read_when_existing_storage_invalid() {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::yubikey::MockDevicePinPolicyPort::new();
        device
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(false));
        let mut secret_input = ports::io::MockSecretInputPort::new();
        let pin_input = ports::io::MockPinInputPort::new();
        secret_input.expect_read_streamed_secret().times(0);

        let mut storage = ports::yubikey::MockSecretStoragePort::new();
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

        let mut report = ports::io::MockReportPort::new();
        report
            .expect_write_verify_report()
            .times(1)
            .withf(|summary| {
                summary.serial == 2001
                    && summary.checks.get(&CheckName::LocalStorage) == Some(&CheckStatus::Failed)
            })
            .returning(|_| Ok(()));

        let result = run_rotate_bws_token_with_stdin(
            RotateBwsTokenCommand { serial: Some(2001) },
            &mut device,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
        );

        assert!(
            result.is_err(),
            "invalid storage must stop before stdin read"
        );
    }

    #[test]
    fn rotate_stdin_reads_pin_when_device_requires_it() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::yubikey::MockDevicePinPolicyPort::new();
        device
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(true));
        let mut secret_input = ports::io::MockSecretInputPort::new();
        secret_input
            .expect_read_streamed_secret()
            .times(1)
            .returning(|| Ok(material(b"new-token")));
        let mut pin_input = ports::io::MockPinInputPort::new();
        pin_input
            .expect_read_pin()
            .times(1)
            .returning(|| Ok(material(b"123456")));
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .returning(|_, _| Ok(write_inspection(false)));
        expect_local_verify_ok(&mut storage, &mut sequence, 2001);
        storage
            .expect_store_secret()
            .times(1)
            .withf(|_, _, secret| secret.len() == b"new-token".len())
            .returning(|_, _, _| Ok(()));
        expect_local_verify_ok(&mut storage, &mut sequence, 2001);
        let mut report = ports::io::MockReportPort::new();
        report
            .expect_write_verify_report()
            .times(1)
            .returning(|_| Ok(()));

        run_rotate_bws_token_with_stdin(
            RotateBwsTokenCommand { serial: Some(2001) },
            &mut device,
            &secret_input,
            &pin_input,
            &mut storage,
            &report,
        )
    }
}
