//! rotate-bws-token(prompt) の順序を固定し、更新手順と検証手順の責任境界を崩さない。

use std::collections::BTreeSet;

use anyhow::bail;

use crate::Result;
use crate::{
    domain::{
        commands::RotateBwsTokenCommand,
        piv::validate_piv_pin_len,
        storage::{
            SecretStorageReadIntent, SecretStorageVerificationPlan, SecretStorageWriteIntent,
        },
        verification::VerifySummary,
    },
    ports,
};

/// `run_rotate_bws_token_with_prompt` が使う外部 capability を named field で束ねる。
pub(crate) struct RotateBwsTokenPromptRuntime<'a> {
    pub(crate) device: &'a mut dyn ports::YubiKeyDevicePort,
    pub(crate) secret_input: &'a dyn ports::SecretInputPort,
    pub(crate) continuation: &'a dyn ports::RotationContinuationPort,
    pub(crate) pin_input: &'a dyn ports::PinInputPort,
    pub(crate) storage: &'a mut dyn ports::SecretStoragePort,
    pub(crate) report: &'a dyn ports::ReportPort,
}

/// prompt 入力で BWS token を更新し、YubiKey 保存状態を再検証する。
///
/// serial 未指定時は port 境界で対象 device を解決し、token 入力前に既存 local storage を
/// read/validate する。更新不能な状態では new token を受け取らない。
pub(crate) fn run_rotate_bws_token_with_prompt(
    command: RotateBwsTokenCommand,
    runtime: RotateBwsTokenPromptRuntime<'_>,
) -> Result<()> {
    let RotateBwsTokenPromptRuntime {
        device,
        secret_input,
        continuation,
        pin_input,
        storage: storage_port,
        report,
    } = runtime;
    let mut updated_serials = BTreeSet::new();
    let mut next_requested_serial = command.serial;
    let mut token = None;

    loop {
        let serial = device.resolve_device_serial(next_requested_serial)?;
        if !updated_serials.insert(serial) {
            bail!("selected YubiKey was already updated");
        }

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

        if token.is_none() {
            token = Some(secret_input.read_bitwarden_client_secret_secret()?);
        }
        let Some(token) = token.as_ref() else {
            bail!("rotate token is unavailable");
        };
        let intent = SecretStorageWriteIntent::store(storage, inspection, token.len())?;
        storage_port.store_secret(serial, intent, token)?;
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
            Ok(()) => report.write_verify_report(&VerifySummary::local_storage_verified(serial))?,
            Err(err) => {
                return report
                    .write_verify_report(&VerifySummary::local_storage_failed(serial))
                    .and(Err(err));
            }
        }

        if command.serial.is_some() || !continuation.continue_rotation()? {
            return Ok(());
        }
        next_requested_serial = None;
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

    use super::{RotateBwsTokenPromptRuntime, run_rotate_bws_token_with_prompt};

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
        storage: &mut ports::MockSecretStoragePort,
        sequence: &mut mockall::Sequence,
        serial: u32,
    ) {
        for name in [
            SecretName::BwEmail,
            SecretName::BwPassword,
            SecretName::BitwardenClientId,
            SecretName::BitwardenClientSecret,
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
                        SecretName::BitwardenClientId | SecretName::BitwardenClientSecret => material(b"access-token"),
                    })
                });
        }
    }

    #[test]
    fn rotate_prompt_verifies_before_token_read_stores_and_reports() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|requested| requested.is_none())
            .returning(|_| Ok(2001));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(false));

        let mut secret_input = ports::MockSecretInputPort::new();
        let mut continuation = ports::MockRotationContinuationPort::new();
        let mut pin_input = ports::MockPinInputPort::new();
        pin_input.expect_read_pin().times(0);
        continuation
            .expect_continue_rotation()
            .times(1)
            .returning(|| Ok(false));

        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|serial, storage| *serial == 2001 && storage.name == SecretName::BitwardenClientSecret)
            .returning(|_, _| Ok(write_inspection(false)));
        expect_local_verify_ok(&mut storage, &mut sequence, 2001);
        secret_input
            .expect_read_bitwarden_client_secret_secret()
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

        run_rotate_bws_token_with_prompt(
            RotateBwsTokenCommand { serial: None },
            RotateBwsTokenPromptRuntime {
                device: &mut (&mut device_serial, &mut pin_policy),
                secret_input: &secret_input,
                continuation: &continuation,
                pin_input: &pin_input,
                storage: &mut storage,
                report: &report,
            },
        )
    }

    #[test]
    fn rotate_prompt_stops_before_token_read_when_existing_storage_invalid() {
        let mut sequence = mockall::Sequence::new();
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(false));
        let mut secret_input = ports::MockSecretInputPort::new();
        let mut continuation = ports::MockRotationContinuationPort::new();
        let pin_input = ports::MockPinInputPort::new();
        secret_input.expect_read_bitwarden_client_secret_secret().times(0);
        continuation.expect_continue_rotation().times(0);

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

        let result = run_rotate_bws_token_with_prompt(
            RotateBwsTokenCommand { serial: Some(2001) },
            RotateBwsTokenPromptRuntime {
                device: &mut (&mut device_serial, &mut pin_policy),
                secret_input: &secret_input,
                continuation: &continuation,
                pin_input: &pin_input,
                storage: &mut storage,
                report: &report,
            },
        );

        assert!(
            result.is_err(),
            "invalid storage must stop before token input"
        );
    }

    #[test]
    fn rotate_prompt_continues_to_another_interactive_device() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .withf(|requested| requested.is_none())
            .returning(|_| Ok(2001));
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .withf(|requested| requested.is_none())
            .returning(|_| Ok(2002));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(2)
            .returning(|_| Ok(false));
        let mut secret_input = ports::MockSecretInputPort::new();
        secret_input
            .expect_read_bitwarden_client_secret_secret()
            .times(1)
            .returning(|| Ok(material(b"new-token")));
        let mut continuation = ports::MockRotationContinuationPort::new();
        continuation
            .expect_continue_rotation()
            .times(1)
            .returning(|| Ok(true));
        continuation
            .expect_continue_rotation()
            .times(1)
            .returning(|| Ok(false));
        let pin_input = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        for serial in [2001, 2002] {
            storage
                .expect_inspect_secret_storage_write()
                .times(1)
                .returning(|_, _| Ok(write_inspection(false)));
            expect_local_verify_ok(&mut storage, &mut sequence, serial);
            storage
                .expect_store_secret()
                .times(1)
                .withf(move |actual_serial, intent, _| {
                    *actual_serial == serial && intent.storage.name == SecretName::BitwardenClientSecret
                })
                .returning(|_, _, _| Ok(()));
            expect_local_verify_ok(&mut storage, &mut sequence, serial);
        }
        let mut report = ports::MockReportPort::new();
        report
            .expect_write_verify_report()
            .times(2)
            .returning(|_| Ok(()));

        run_rotate_bws_token_with_prompt(
            RotateBwsTokenCommand { serial: None },
            RotateBwsTokenPromptRuntime {
                device: &mut (&mut device_serial, &mut pin_policy),
                secret_input: &secret_input,
                continuation: &continuation,
                pin_input: &pin_input,
                storage: &mut storage,
                report: &report,
            },
        )
    }

    #[test]
    fn rotate_prompt_rejects_already_updated_device() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Ok(2001));
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Ok(2001));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(false));
        let mut secret_input = ports::MockSecretInputPort::new();
        secret_input
            .expect_read_bitwarden_client_secret_secret()
            .times(1)
            .returning(|| Ok(material(b"new-token")));
        let mut continuation = ports::MockRotationContinuationPort::new();
        continuation
            .expect_continue_rotation()
            .times(1)
            .returning(|| Ok(true));
        let pin_input = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .returning(|_, _| Ok(write_inspection(false)));
        let mut sequence = mockall::Sequence::new();
        expect_local_verify_ok(&mut storage, &mut sequence, 2001);
        storage
            .expect_store_secret()
            .times(1)
            .returning(|_, _, _| Ok(()));
        expect_local_verify_ok(&mut storage, &mut sequence, 2001);
        let mut report = ports::MockReportPort::new();
        report
            .expect_write_verify_report()
            .times(1)
            .returning(|_| Ok(()));

        let result = run_rotate_bws_token_with_prompt(
            RotateBwsTokenCommand { serial: None },
            RotateBwsTokenPromptRuntime {
                device: &mut (&mut device_serial, &mut pin_policy),
                secret_input: &secret_input,
                continuation: &continuation,
                pin_input: &pin_input,
                storage: &mut storage,
                report: &report,
            },
        );

        assert!(result.is_err(), "same device must not be rotated twice");
    }
}
