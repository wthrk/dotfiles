//! `put` の PIV management lifecycle を、secret の入力 source から分離する。

use crate::{
    Result,
    features::yubikey_lifecycle::domain::commands::PutCommand,
    features::yubikey_lifecycle::ports::public::piv_pin_input::PivPinInputPort,
    features::{
        cli_interaction::ports::public::BitwardenClientSecretInputPort,
        yubikey_lifecycle::{
            domain::storage::SecretStorageWriteIntent,
            ports::{DeviceSerialPort, SecretStoragePort},
        },
    },
};

/// `bitwarden-client-secret` を保存する。
pub(crate) fn run_put(
    command: PutCommand,
    device: &mut dyn DeviceSerialPort,
    piv_pin: &dyn PivPinInputPort,
    secret_input: &dyn BitwardenClientSecretInputPort,
    storage: &mut dyn SecretStoragePort,
) -> Result<()> {
    let serial = device.resolve_device_serial(command.serial)?;
    let pin = piv_pin.read_piv_pin_secret()?;
    storage.begin_piv_management_session(serial, pin)?;

    let storage_spec = command.storage_spec(serial);
    let inspection = storage.inspect_secret_storage_write(serial, &storage_spec)?;
    let _preflight =
        SecretStorageWriteIntent::preflight_put(storage_spec.clone(), &inspection, command.force)?;
    let secret = secret_input.read_bitwarden_client_secret()?;
    let intent =
        SecretStorageWriteIntent::put(storage_spec, inspection, command.force, secret.len())?;
    storage.store_secret(serial, intent, &secret)
}

#[cfg(test)]
mod tests {
    use crate::{
        features::yubikey_lifecycle::domain::commands::PutCommand,
        features::{
            cli_interaction::ports::public::MockBitwardenClientSecretInputPort,
            yubikey_lifecycle::{
                domain::{
                    manifest::SecretManifest,
                    piv::SecretName,
                    storage::{SecretStorageWriteInspection, SecretStorageWriteIntent},
                },
                ports::public::{MockDeviceSerialPort, MockSecretStoragePort},
            },
        },
        foundation::protection::ProtectedSecret,
    };

    use super::run_put;

    fn configure_management_device_fixture(device: &mut MockDeviceSerialPort) {
        let _ = device;
    }

    fn material(bytes: &'static [u8]) -> crate::Result<ProtectedSecret> {
        ProtectedSecret::from_test_bytes(bytes)
    }

    fn inspection(object_exists: bool) -> crate::Result<SecretStorageWriteInspection> {
        Ok(SecretStorageWriteInspection {
            manifest_present: true,
            manifest_bytes: Some(SecretManifest::fixture_v2().encode()?),
            object_present: object_exists,
            object_exists,
            reserved_slot_key_exists: true,
            reserved_slot_certificate_exists: false,
            slot_public_key_spki: SecretManifest::fixture_v2().slot_public_key_spki,
        })
    }

    #[test]
    fn put_runner_stops_before_reader_when_preflight_rejects_write() -> crate::Result<()> {
        let mut device = MockDeviceSerialPort::new();
        configure_management_device_fixture(&mut device);
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut pin = crate::features::yubikey_lifecycle::ports::public::piv_pin_input::MockPivPinInputPort::new();
        pin.expect_read_piv_pin_secret()
            .returning(|| material(b"123456"));
        let mut input = MockBitwardenClientSecretInputPort::new();
        input.expect_read_bitwarden_client_secret().times(0);
        let mut storage = MockSecretStoragePort::new();
        storage
            .expect_begin_piv_management_session()
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_write()
            .returning(|_, _| inspection(true));
        storage.expect_store_secret().times(0);

        assert!(
            run_put(
                PutCommand {
                    serial: Some(2001),
                    name: SecretName::BitwardenClientSecret,
                    force: false,
                },
                &mut device,
                &pin,
                &input,
                &mut storage,
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn put_force_rejects_zero_length_manifest_partial_before_secret_input() -> crate::Result<()> {
        let mut device = MockDeviceSerialPort::new();
        configure_management_device_fixture(&mut device);
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut pin = crate::features::yubikey_lifecycle::ports::public::piv_pin_input::MockPivPinInputPort::new();
        pin.expect_read_piv_pin_secret()
            .returning(|| material(b"123456"));
        let mut input = MockBitwardenClientSecretInputPort::new();
        input.expect_read_bitwarden_client_secret().never();
        let mut storage = MockSecretStoragePort::new();
        storage
            .expect_begin_piv_management_session()
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_write()
            .returning(|_, _| {
                Ok(SecretStorageWriteInspection {
                    manifest_present: true,
                    manifest_bytes: None,
                    object_present: true,
                    object_exists: false,
                    reserved_slot_key_exists: true,
                    reserved_slot_certificate_exists: false,
                    slot_public_key_spki: None,
                })
            });
        storage.expect_store_secret().never();

        assert!(
            run_put(
                PutCommand {
                    serial: Some(2001),
                    name: SecretName::BitwardenClientSecret,
                    force: true,
                },
                &mut device,
                &pin,
                &input,
                &mut storage,
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn put_runner_accepts_a_streamed_reader_without_a_second_lifecycle() -> crate::Result<()> {
        let mut device = MockDeviceSerialPort::new();
        configure_management_device_fixture(&mut device);
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut pin = crate::features::yubikey_lifecycle::ports::public::piv_pin_input::MockPivPinInputPort::new();
        pin.expect_read_piv_pin_secret()
            .returning(|| material(b"123456"));
        let mut input = MockBitwardenClientSecretInputPort::new();
        input
            .expect_read_bitwarden_client_secret()
            .times(1)
            .returning(|| material(b"token"));
        let mut storage = MockSecretStoragePort::new();
        storage
            .expect_begin_piv_management_session()
            .times(1)
            .withf(|serial, _| *serial == 2001)
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .returning(|_, _| inspection(false));
        storage
            .expect_store_secret()
            .times(1)
            .withf(
                |serial: &u32, intent: &SecretStorageWriteIntent, secret: &ProtectedSecret| {
                    *serial == 2001
                        && intent.storage.name == SecretName::BitwardenClientSecret
                        && secret.len() == b"token".len()
                },
            )
            .returning(|_, _, _| Ok(()));

        run_put(
            PutCommand {
                serial: None,
                name: SecretName::BitwardenClientSecret,
                force: false,
            },
            &mut device,
            &pin,
            &input,
            &mut storage,
        )
    }
}
