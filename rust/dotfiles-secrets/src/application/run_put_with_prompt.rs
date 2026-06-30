//! put(prompt) の順序責務を保持し、入力手段と保存実装の変更理由を usecase から分離する。

use crate::Result;
use crate::{
    domain::{commands::PutCommand, storage::SecretStorageWriteIntent},
    ports,
};

/// 対話入力で取得した secret を対象 serial の YubiKey storage へ保存する。
///
/// storage preflight を入力より先に完了し、保存不能な状態では secret を取得しない。入力手段と
/// 保護 buffer 化は `SecretInputPort` 境界へ閉じ、use case は対象 secret の選択と保存順序だけを担う。
pub(crate) async fn run_put_with_prompt<D, P, S>(
    command: PutCommand,
    device: &mut D,
    process: &P,
    storage_port: &mut S,
) -> Result<()>
where
    D: ports::yubikey::DeviceSerialPort,
    P: ports::io::SecretInputPort,
    S: ports::yubikey::SecretStoragePort,
{
    let serial = device.resolve_device_serial()?;
    let storage = command.storage_spec(serial);
    let inspection = storage_port.inspect_secret_storage_write(serial, &storage)?;
    SecretStorageWriteIntent::ensure_put_preconditions(&storage, &inspection, command.force)?;
    let secret = command.name.read_interactive_secret_with(
        || process.read_bitwarden_client_id_secret(),
        || process.read_bitwarden_client_secret(),
    )?;
    let intent = SecretStorageWriteIntent::put(storage, inspection, command.force, secret.len())?;
    storage_port.store_secret(serial, intent, &secret)
}

#[cfg(test)]
/// `put` use case の preflight、secret 入力、保存呼び出し順序を port mock で固定する。
mod tests {
    use super::run_put_with_prompt;
    use crate::{
        domain::{
            commands::PutCommand, manifest::SecretManifest, piv::SecretName,
            storage::SecretStorageWriteInspection,
        },
        ports,
        support::protection::ProtectedSecret,
    };

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn write_inspection(object_exists: bool) -> SecretStorageWriteInspection {
        SecretStorageWriteInspection {
            manifest_bytes: Some(SecretManifest::expected().encode().expect("manifest")),
            object_exists,
        }
    }

    /// storage preflight が失敗した場合、secret input port を呼ばずに停止する。
    #[tokio::test]
    async fn put_prompt_stops_before_input_when_storage_preflight_fails() {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::yubikey::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(2001));
        let mut process = ports::io::MockSecretInputPort::new();
        process.expect_read_bitwarden_client_id_secret().times(0);
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(write_inspection(true)));
        storage.expect_store_secret().times(0);

        let result = run_put_with_prompt(
            PutCommand {
                name: SecretName::BitwardenClientId,
                force: false,
            },
            &mut device,
            &process,
            &mut storage,
        )
        .await;

        assert!(result.is_err());
    }

    /// 指定された secret name だけを input port から読み、同じ内容長の保存 intent で storage へ渡す。
    #[tokio::test]
    async fn put_prompt_reads_requested_secret_and_stores_it() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::yubikey::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(2001));
        let mut process = ports::io::MockSecretInputPort::new();
        process.expect_read_bitwarden_client_id_secret().times(0);
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(write_inspection(false)));
        process
            .expect_read_bitwarden_client_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(b"password")));
        storage
            .expect_store_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|serial, intent, secret| {
                *serial == 2001
                    && intent.storage.name == SecretName::BitwardenClientSecret
                    && secret.len() == b"password".len()
            })
            .returning(|_, _, _| Ok(()));

        run_put_with_prompt(
            PutCommand {
                name: SecretName::BitwardenClientSecret,
                force: false,
            },
            &mut device,
            &process,
            &mut storage,
        )
        .await
    }
}
