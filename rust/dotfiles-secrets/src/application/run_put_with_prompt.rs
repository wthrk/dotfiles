//! put(prompt) の順序責務を保持し、入力手段と保存実装の変更理由を usecase から分離する。

use crate::Result;
use crate::{
    domain::{commands::PutCommand, storage::SecretStorageWriteIntent},
    ports,
};

/// 対話入力で取得した secret を対象 serial の YubiKey storage へ保存する。
///
/// 入力モードの可視/不可視判定は `SecretName` の domain 規則で決め、端末 I/O 実装詳細は adapter へ委譲する。
pub(crate) fn run_put_with_prompt<D, P, S>(
    command: PutCommand,
    device: &mut D,
    process: &P,
    storage_port: &mut S,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    P: ports::SecretInputPort,
    S: ports::SecretStoragePort,
{
    let serial = device.resolve_device_serial(command.serial)?;
    let storage = command.storage_spec(serial);
    let inspection = storage_port.inspect_secret_storage_write(serial, &storage)?;
    let _preflight =
        SecretStorageWriteIntent::preflight_put(storage.clone(), &inspection, command.force)?;
    let secret = command.name.read_interactive_secret_with(
        || process.read_bw_email_secret(),
        || process.read_bw_password_secret(),
        || process.read_bitwarden_client_id_secret(),
        || process.read_bitwarden_client_secret_secret(),
    )?;
    let intent = SecretStorageWriteIntent::put(storage, inspection, command.force, secret.len())?;
    storage_port.store_secret(serial, intent, &secret)
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

    use super::run_put_with_prompt;

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn write_inspection(object_exists: bool) -> SecretStorageWriteInspection {
        SecretStorageWriteInspection {
            manifest_bytes: Some(SecretManifest::fixture_v2().encode().expect("manifest")),
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

    #[test]
    fn put_prompt_checks_storage_before_reading_secret() {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        let mut process = ports::MockSecretInputPort::new();
        process.expect_read_bitwarden_client_id_secret().times(0);
        process
            .expect_read_bitwarden_client_secret_secret()
            .times(0);
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(write_inspection(true)));
        storage.expect_store_secret().times(0);

        let result = run_put_with_prompt(
            PutCommand {
                serial: Some(2001),
                name: SecretName::BitwardenClientSecret,
                force: false,
            },
            &mut device,
            &process,
            &mut storage,
        );

        assert!(
            result.is_err(),
            "storage precondition failure must stop before prompt input"
        );
    }

    #[test]
    fn put_prompt_stores_requested_secret() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        let mut process = ports::MockSecretInputPort::new();
        process.expect_read_bw_email_secret().times(0);
        process.expect_read_bw_password_secret().times(0);
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(write_inspection(false)));
        process
            .expect_read_bitwarden_client_secret_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(b"token")));
        storage
            .expect_store_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|serial, intent, secret| {
                *serial == 2001
                    && intent.storage.name == SecretName::BitwardenClientSecret
                    && secret.len() == b"token".len()
            })
            .returning(|_, _, _| Ok(()));

        run_put_with_prompt(
            PutCommand {
                serial: Some(2001),
                name: SecretName::BitwardenClientSecret,
                force: false,
            },
            &mut device,
            &process,
            &mut storage,
        )
    }

    #[test]
    fn put_prompt_stops_when_secret_read_fails() {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(write_inspection(false)));
        storage.expect_store_secret().times(0);
        let mut process = ports::MockSecretInputPort::new();
        process
            .expect_read_bitwarden_client_secret_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Err(anyhow::anyhow!("prompt failed")));

        let result = run_put_with_prompt(
            PutCommand {
                serial: Some(2001),
                name: SecretName::BitwardenClientSecret,
                force: false,
            },
            &mut device,
            &process,
            &mut storage,
        );

        assert!(result.is_err(), "prompt failure must stop before store");
    }
}
