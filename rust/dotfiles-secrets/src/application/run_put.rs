//! `put` の PIV management lifecycle を、secret の入力 source から分離する。

use crate::{
    Result,
    domain::{commands::PutCommand, storage::SecretStorageWriteIntent},
    ports,
};

/// `bitwarden-client-secret` を保存する。
pub(crate) fn run_put(
    command: PutCommand,
    device: &mut dyn ports::DeviceSerialPort,
    piv_pin: &dyn ports::PivPinInputPort,
    secret_input: &dyn ports::BitwardenClientSecretInputPort,
    storage: &mut dyn ports::SecretStoragePort,
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
        domain::{
            commands::PutCommand, manifest::SecretManifest, piv::SecretName,
            storage::SecretStorageWriteInspection,
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_put;

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn inspection(object_exists: bool) -> SecretStorageWriteInspection {
        SecretStorageWriteInspection {
            manifest_bytes: Some(SecretManifest::fixture_v2().encode().expect("manifest")),
            object_present: object_exists,
            object_exists,
            reserved_slot_key_exists: true,
            reserved_slot_certificate_exists: false,
            slot_public_key_spki: Some(
                SecretManifest::fixture_v2()
                    .slot_public_key_spki
                    .expect("fixture SPKI"),
            ),
        }
    }

    #[test]
    fn put_runner_stops_before_reader_when_preflight_rejects_write() {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut pin = ports::io::MockPivPinInputPort::new();
        pin.expect_read_piv_pin_secret()
            .returning(|| Ok(material(b"123456")));
        let mut input = ports::MockBitwardenClientSecretInputPort::new();
        input.expect_read_bitwarden_client_secret().times(0);
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_management_session()
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_write()
            .returning(|_, _| Ok(inspection(true)));
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
    }

    #[test]
    fn put_runner_accepts_a_streamed_reader_without_a_second_lifecycle() -> crate::Result<()> {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut pin = ports::io::MockPivPinInputPort::new();
        pin.expect_read_piv_pin_secret()
            .returning(|| Ok(material(b"123456")));
        let mut input = ports::MockBitwardenClientSecretInputPort::new();
        input
            .expect_read_bitwarden_client_secret()
            .times(1)
            .returning(|| Ok(material(b"token")));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_management_session()
            .times(1)
            .withf(|serial, _| *serial == 2001)
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .returning(|_, _| Ok(inspection(false)));
        storage
            .expect_store_secret()
            .times(1)
            .withf(|serial, intent, secret| {
                *serial == 2001
                    && intent.storage.name == SecretName::BitwardenClientSecret
                    && secret.len() == b"token".len()
            })
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
