//! rotate-bws-token(prompt) の順序を固定し、更新手順と検証手順の責任境界を崩さない。

use std::collections::BTreeSet;

use anyhow::bail;

use crate::Result;
use crate::{
    features::provisioning_verification::domain::commands::RotateBwsTokenCommand,
    features::{
        cli_interaction::ports::public::{
            BitwardenClientSecretInputPort, ReportPort, RotationContinuationPort,
        },
        provisioning_verification::domain::verification::VerifySummary,
        yubikey_lifecycle::{
            domain::storage::{
                SecretStorageReadIntent, SecretStorageVerificationPlan, SecretStorageWriteIntent,
            },
            ports::public::piv_pin_input::PivPinInputPort,
            ports::{DeviceSerialPort, SecretStoragePort},
        },
    },
};

/// BWS token を更新し、YubiKey 保存状態を再検証する。
///
/// serial 未指定時は各更新ステップで port 境界から単一接続 device を解決し、token 入力前に
/// 既存 local storage を read/validate する。更新不能な状態では new token を受け取らない。
pub(crate) fn run_rotate_bws_token(
    command: RotateBwsTokenCommand,
    device: &mut dyn DeviceSerialPort,
    piv_pin: &dyn PivPinInputPort,
    token_input: &dyn BitwardenClientSecretInputPort,
    continuation: &dyn RotationContinuationPort,
    storage_port: &mut dyn SecretStoragePort,
    report: &dyn ReportPort,
) -> Result<()> {
    let mut updated_serials = BTreeSet::new();
    let mut next_requested_serial = command.serial;
    let mut token = None;

    loop {
        let serial = device.resolve_device_serial(next_requested_serial)?;
        device
            .inspect_device_profile(serial)?
            .ensure_pin_free_recovery_supported()?;
        if !updated_serials.insert(serial) {
            bail!("selected YubiKey was already updated");
        }
        // A serial is resolved before its PIN is read. The first PIN starts the session;
        // every later YubiKey requires a fresh hidden-TTY PIN and a separate PIV session.
        let pin = piv_pin.read_piv_pin_secret()?;
        if updated_serials.len() == 1 {
            storage_port.begin_piv_management_session(serial, pin)?;
        } else {
            storage_port.begin_next_piv_management_session(serial, pin)?;
        }

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

        if token.is_none() {
            token = Some(token_input.read_bitwarden_client_secret()?);
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
                    .load_secret(serial, &intent)
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
        features::{
            provisioning_verification::domain::{
                commands::RotateBwsTokenCommand,
                verification::{CheckName, CheckStatus},
            },
            yubikey_lifecycle::domain::{
                self as domain,
                manifest::SecretManifest,
                piv::SecretName,
                storage::{SecretStorageReadInspection, SecretStorageWriteInspection},
            },
        },
        foundation::protection::ProtectedSecret,
    };
    mod ports {
        pub(crate) use crate::features::cli_interaction::ports::public::{
            MockBitwardenClientSecretInputPort, MockReportPort, MockRotationContinuationPort,
        };
        pub(crate) mod io {
            pub(crate) use crate::features::yubikey_lifecycle::ports::public::piv_pin_input::MockPivPinInputPort;
        }
    }

    use super::run_rotate_bws_token;

    fn expect_pin_free_device_profile(
        device: &mut crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort,
    ) {
        device.expect_inspect_device_profile().returning(|_| {
            Ok(domain::piv::PivDeviceProfile {
                version: domain::piv::PivApplicationVersion {
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

    fn manifest() -> crate::Result<Vec<u8>> {
        SecretManifest::fixture_v2().encode()
    }

    fn write_inspection(object_exists: bool) -> crate::Result<SecretStorageWriteInspection> {
        Ok(SecretStorageWriteInspection {
            manifest_present: true,
            manifest_bytes: Some(manifest()?),
            object_present: object_exists,
            object_exists,
            reserved_slot_key_exists: true,
            reserved_slot_certificate_exists: false,
            slot_public_key_spki: SecretManifest::fixture_v2().slot_public_key_spki,
        })
    }

    fn read_inspection(encoded: bool) -> crate::Result<SecretStorageReadInspection> {
        Ok(SecretStorageReadInspection {
            manifest_bytes: Some(manifest()?),
            encoded: encoded.then_some(vec![1]),
        })
    }

    fn expect_local_verify_ok(
        storage: &mut crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort,
        sequence: &mut mockall::Sequence,
        serial: u32,
    ) {
        let name = SecretName::BitwardenClientSecret;
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .in_sequence(sequence)
            .withf(move |actual_serial, storage| *actual_serial == serial && storage.name == name)
            .returning(|_, _| read_inspection(true));
        storage
            .expect_load_secret()
            .times(1)
            .in_sequence(sequence)
            .withf(move |actual_serial, intent| {
                *actual_serial == serial && intent.storage.name == name
            })
            .returning(|_, _| material(b"access-token"));
    }

    #[test]
    fn rotate_streamed_token_uses_the_same_piv_lifecycle_and_reports() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device_serial =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        expect_pin_free_device_profile(&mut device_serial);
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|requested| requested.is_none())
            .returning(|_| Ok(2001));
        let mut secret_input = ports::MockBitwardenClientSecretInputPort::new();
        let mut piv_pin = ports::io::MockPivPinInputPort::new();
        piv_pin
            .expect_read_piv_pin_secret()
            .times(1)
            .returning(|| material(b"123456"));
        let mut continuation = ports::MockRotationContinuationPort::new();
        continuation
            .expect_continue_rotation()
            .times(1)
            .returning(|| Ok(false));

        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_management_session()
            .times(1)
            .withf(|serial, _| *serial == 2001)
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|serial, storage| {
                *serial == 2001 && storage.name == SecretName::BitwardenClientSecret
            })
            .returning(|_, _| write_inspection(false));
        expect_local_verify_ok(&mut storage, &mut sequence, 2001);
        secret_input
            .expect_read_bitwarden_client_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| material(b"new-token"));
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

        run_rotate_bws_token(
            RotateBwsTokenCommand { serial: None },
            &mut device_serial,
            &piv_pin,
            &secret_input,
            &continuation,
            &mut storage,
            &report,
        )
    }

    #[test]
    fn rotate_prompt_stops_before_token_read_when_existing_storage_invalid() {
        let mut sequence = mockall::Sequence::new();
        let mut device_serial =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        expect_pin_free_device_profile(&mut device_serial);
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        let mut secret_input = ports::MockBitwardenClientSecretInputPort::new();
        let mut piv_pin = ports::io::MockPivPinInputPort::new();
        piv_pin
            .expect_read_piv_pin_secret()
            .times(1)
            .returning(|| material(b"123456"));
        let mut continuation = ports::MockRotationContinuationPort::new();
        secret_input.expect_read_bitwarden_client_secret().times(0);
        continuation.expect_continue_rotation().times(0);

        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_management_session()
            .times(1)
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| write_inspection(false));
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| read_inspection(false));
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

        let result = run_rotate_bws_token(
            RotateBwsTokenCommand { serial: Some(2001) },
            &mut device_serial,
            &piv_pin,
            &secret_input,
            &continuation,
            &mut storage,
            &report,
        );

        assert!(
            result.is_err(),
            "invalid storage must stop before token input"
        );
    }

    #[test]
    fn rotate_prompt_reuses_token_but_requires_a_fresh_pin_for_each_next_device()
    -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device_serial =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        expect_pin_free_device_profile(&mut device_serial);
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
        let mut secret_input = ports::MockBitwardenClientSecretInputPort::new();
        let mut piv_pin = ports::io::MockPivPinInputPort::new();
        piv_pin
            .expect_read_piv_pin_secret()
            .times(2)
            .returning(|| material(b"123456"));
        secret_input
            .expect_read_bitwarden_client_secret()
            .times(1)
            .returning(|| material(b"new-token"));
        let mut continuation = ports::MockRotationContinuationPort::new();
        continuation
            .expect_continue_rotation()
            .times(1)
            .returning(|| Ok(true));
        continuation
            .expect_continue_rotation()
            .times(1)
            .returning(|| Ok(false));
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_management_session()
            .times(1)
            .withf(|serial, _| *serial == 2001)
            .returning(|_, _| Ok(()));
        storage
            .expect_begin_next_piv_management_session()
            .times(1)
            .withf(|serial, _| *serial == 2002)
            .returning(|_, _| Ok(()));
        for serial in [2001, 2002] {
            storage
                .expect_inspect_secret_storage_write()
                .times(1)
                .returning(|_, _| write_inspection(false));
            expect_local_verify_ok(&mut storage, &mut sequence, serial);
            storage
                .expect_store_secret()
                .times(1)
                .withf(move |actual_serial, intent, _| {
                    *actual_serial == serial
                        && intent.storage.name == SecretName::BitwardenClientSecret
                })
                .returning(|_, _, _| Ok(()));
            expect_local_verify_ok(&mut storage, &mut sequence, serial);
        }
        let mut report = ports::MockReportPort::new();
        report
            .expect_write_verify_report()
            .times(2)
            .returning(|_| Ok(()));

        run_rotate_bws_token(
            RotateBwsTokenCommand { serial: None },
            &mut device_serial,
            &piv_pin,
            &secret_input,
            &continuation,
            &mut storage,
            &report,
        )
    }

    #[test]
    fn rotate_prompt_rejects_already_updated_device() {
        let mut device_serial =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        expect_pin_free_device_profile(&mut device_serial);
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Ok(2001));
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Ok(2001));
        let mut secret_input = ports::MockBitwardenClientSecretInputPort::new();
        let mut piv_pin = ports::io::MockPivPinInputPort::new();
        piv_pin
            .expect_read_piv_pin_secret()
            .times(1)
            .returning(|| material(b"123456"));
        secret_input
            .expect_read_bitwarden_client_secret()
            .times(1)
            .returning(|| material(b"new-token"));
        let mut continuation = ports::MockRotationContinuationPort::new();
        continuation
            .expect_continue_rotation()
            .times(1)
            .returning(|| Ok(true));
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_management_session()
            .times(1)
            .returning(|_, _| Ok(()));
        storage.expect_begin_next_piv_management_session().times(0);
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .returning(|_, _| write_inspection(false));
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

        let result = run_rotate_bws_token(
            RotateBwsTokenCommand { serial: None },
            &mut device_serial,
            &piv_pin,
            &secret_input,
            &continuation,
            &mut storage,
            &report,
        );

        assert!(result.is_err(), "same device must not be rotated twice");
    }
}
