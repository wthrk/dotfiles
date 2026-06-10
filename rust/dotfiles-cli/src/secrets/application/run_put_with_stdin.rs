//! put(stdin) の順序責務を保持し、stdin 読み取り実装と storage 実装の境界を固定する。

use crate::Result;
use crate::secrets::{
    domain::{commands::PutCommand, piv::SecretName, storage::SecretStorageWriteIntent},
    ports,
};

/// stdin から受け取った secret を選択済み YubiKey storage へ保存する。
///
/// use case は device 選択、入力取得、保存の順序のみを担い、stdin 条件やサイズ制約は adapter 実装側へ
/// 閉じ込める。stdin が pipe の場合でも device 選択は `DeviceSerialPort` へ委譲し、複数接続時の停止も
/// その境界に閉じ込める。
pub(crate) async fn run_put_with_stdin<D, P, S, B>(
    command: PutCommand,
    device: &mut D,
    process: &P,
    storage_port: &mut S,
    bws_client: &B,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    P: ports::SecretInputPort,
    S: ports::SecretStoragePort,
    B: ports::BwsClientPort,
{
    let serial = device.resolve_device_serial()?;
    let storage = command.storage_spec(serial);
    let inspection = storage_port.inspect_secret_storage_write(serial, &storage)?;
    SecretStorageWriteIntent::ensure_put_preconditions(&storage, &inspection, command.force)?;
    let secret = process.read_streamed_secret()?;
    if command.name == SecretName::BwsAccessToken {
        bws_client.ensure_recovery_token_provenance(&secret).await?;
    }
    let intent = SecretStorageWriteIntent::put(storage, inspection, command.force, secret.len())?;
    storage_port.store_secret(serial, intent, &secret)
}

#[cfg(test)]
mod tests {
    use crate::secrets::{
        domain::{
            commands::PutCommand, manifest::SecretManifest, piv::SecretName,
            storage::SecretStorageWriteInspection,
        },
        ports,
        ports::ProtectedSecret,
    };

    use super::run_put_with_stdin;

    fn write_inspection(object_exists: bool) -> SecretStorageWriteInspection {
        SecretStorageWriteInspection {
            manifest_bytes: Some(SecretManifest::expected().encode().expect("manifest")),
            object_exists,
        }
    }

    #[tokio::test]
    async fn put_stdin_checks_storage_before_reading_secret() {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2001));
        let process = ports::MockSecretInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        let bws = ports::MockBwsClientPort::new();
        let mut sequence = mockall::Sequence::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(write_inspection(true)));
        storage.expect_store_secret().times(0);

        let result = run_put_with_stdin(
            PutCommand {
                name: SecretName::BwsAccessToken,
                force: false,
            },
            &mut device,
            &process,
            &mut storage,
            &bws,
        )
        .await;

        assert!(
            result.is_err(),
            "preflight failure must stop before stdin read"
        );
    }

    #[tokio::test]
    async fn put_stdin_stores_requested_secret() -> crate::Result<()> {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2001));
        let mut process = ports::MockSecretInputPort::new();
        process
            .expect_read_streamed_secret()
            .times(1)
            .returning(|| {
                Ok(ProtectedSecret::from_test_bytes(b"recovery-token").expect("test secret"))
            });
        let mut storage = ports::MockSecretStoragePort::new();
        let mut bws = ports::MockBwsClientPort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .returning(|_, _| Ok(write_inspection(false)));
        bws.expect_ensure_recovery_token_provenance()
            .times(1)
            .returning(|_| Ok(()));
        storage
            .expect_store_secret()
            .times(1)
            .withf(|serial, intent, _| {
                *serial == 2001 && intent.storage.name == SecretName::BwsAccessToken
            })
            .returning(|_, _, _| Ok(()));

        run_put_with_stdin(
            PutCommand {
                name: SecretName::BwsAccessToken,
                force: false,
            },
            &mut device,
            &process,
            &mut storage,
            &bws,
        )
        .await
    }

    #[tokio::test]
    async fn put_stdin_rejects_same_token_as_provisioning() {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2001));
        let mut process = ports::MockSecretInputPort::new();
        process
            .expect_read_streamed_secret()
            .times(1)
            .returning(
                || Ok(ProtectedSecret::from_test_bytes(b"same-token").expect("test secret")),
            );
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .returning(|_, _| Ok(write_inspection(false)));
        storage.expect_store_secret().times(0);
        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_ensure_recovery_token_provenance()
            .times(1)
            .returning(|_| {
                Err(anyhow::anyhow!(
                    "refusing to store bws-access-token: recovery token must differ from the provisioning token"
                ))
            });

        let result = run_put_with_stdin(
            PutCommand {
                name: SecretName::BwsAccessToken,
                force: false,
            },
            &mut device,
            &process,
            &mut storage,
            &bws,
        )
        .await;

        assert!(result.is_err(), "same provisioning token must be rejected");
    }

    #[tokio::test]
    async fn put_stdin_rejects_missing_or_invalid_provenance_note() {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2001));
        let mut process = ports::MockSecretInputPort::new();
        process
            .expect_read_streamed_secret()
            .times(1)
            .returning(|| {
                Ok(ProtectedSecret::from_test_bytes(b"candidate-token").expect("test secret"))
            });
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .returning(|_, _| Ok(write_inspection(false)));
        storage.expect_store_secret().times(0);
        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_ensure_recovery_token_provenance()
            .times(1)
            .returning(|_| {
                Err(anyhow::anyhow!(
                    "refusing to store bws-access-token: password-store-remote is missing provisioning token provenance"
                ))
            });

        let result = run_put_with_stdin(
            PutCommand {
                name: SecretName::BwsAccessToken,
                force: false,
            },
            &mut device,
            &process,
            &mut storage,
            &bws,
        )
        .await;

        assert_eq!(
            result
                .expect_err("tampered provenance note must be rejected")
                .to_string(),
            "refusing to store bws-access-token: password-store-remote is missing provisioning token provenance"
        );
    }
}
