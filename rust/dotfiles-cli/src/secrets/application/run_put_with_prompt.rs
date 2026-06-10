//! put(prompt) の順序責務を保持し、入力手段と保存実装の変更理由を usecase から分離する。

use crate::Result;
use crate::secrets::{
    domain::{commands::PutCommand, piv::SecretName, storage::SecretStorageWriteIntent},
    ports,
};

/// 対話入力で取得した secret を対象 serial の YubiKey storage へ保存する。
///
/// 入力モードの可視/不可視判定は `SecretName` の domain 規則で決め、端末 I/O 実装詳細は adapter へ委譲する。
pub(crate) async fn run_put_with_prompt<D, P, S, B>(
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
    let secret = command.name.read_interactive_secret_with(
        || process.read_bw_email_secret(),
        || process.read_bw_password_secret(),
        || process.read_bws_access_token_secret(),
    )?;
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

    use super::run_put_with_prompt;

    const TOKEN_ENCRYPTION_KEY: &str = "X8vbvA0bduihIDe/qrzIQQ==";

    fn bws_token(id: &str, client_secret: &str) -> ProtectedSecret {
        let token = format!("0.{id}.{client_secret}:{TOKEN_ENCRYPTION_KEY}");
        ProtectedSecret::from_test_bytes(token.as_bytes()).expect("test token")
    }

    fn write_inspection(object_exists: bool) -> SecretStorageWriteInspection {
        SecretStorageWriteInspection {
            manifest_bytes: Some(SecretManifest::expected().encode().expect("manifest")),
            object_exists,
        }
    }

    #[tokio::test]
    async fn put_prompt_checks_storage_before_reading_secret() {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(2001));
        let mut process = ports::MockSecretInputPort::new();
        process.expect_read_bws_access_token_secret().times(0);
        let mut storage = ports::MockSecretStoragePort::new();
        let bws = ports::MockBwsClientPort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(write_inspection(true)));
        storage.expect_store_secret().times(0);

        let result = run_put_with_prompt(
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
            "storage precondition failure must stop before prompt input"
        );
    }

    #[tokio::test]
    async fn put_prompt_stores_requested_secret() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(2001));
        let mut process = ports::MockSecretInputPort::new();
        process.expect_read_bw_email_secret().times(0);
        process.expect_read_bw_password_secret().times(0);
        let mut storage = ports::MockSecretStoragePort::new();
        let mut bws = ports::MockBwsClientPort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(write_inspection(false)));
        process
            .expect_read_bws_access_token_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| {
                Ok(bws_token(
                    "ec2c1d46-6a4b-4751-a310-af9601317f2d",
                    "recoverySecret123",
                ))
            });
        bws.expect_ensure_recovery_token_provenance()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(()));
        storage
            .expect_store_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|serial, intent, secret| {
                *serial == 2001
                    && intent.storage.name == SecretName::BwsAccessToken
                    && secret.len()
                        == format!(
                            "0.{}.{}:{}",
                            "ec2c1d46-6a4b-4751-a310-af9601317f2d",
                            "recoverySecret123",
                            TOKEN_ENCRYPTION_KEY
                        )
                        .len()
            })
            .returning(|_, _, _| Ok(()));

        run_put_with_prompt(
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
    async fn put_prompt_stops_when_secret_read_fails() {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        let bws = ports::MockBwsClientPort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(write_inspection(false)));
        storage.expect_store_secret().times(0);
        let mut process = ports::MockSecretInputPort::new();
        process
            .expect_read_bws_access_token_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Err(anyhow::anyhow!("prompt failed")));

        let result = run_put_with_prompt(
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

        assert!(result.is_err(), "prompt failure must stop before store");
    }

    #[tokio::test]
    async fn put_prompt_rejects_same_token_as_provisioning() {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(write_inspection(false)));
        storage.expect_store_secret().times(0);
        let mut process = ports::MockSecretInputPort::new();
        process
            .expect_read_bws_access_token_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| {
                Ok(bws_token(
                    "f706285d-46a0-49f7-b440-8b7dbd2e5d79",
                    "sameTokenSecret123",
                ))
            });
        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_ensure_recovery_token_provenance()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| {
                Err(anyhow::anyhow!(
                    "refusing to store bws-access-token: recovery token must differ from the provisioning token"
                ))
            });

        let result = run_put_with_prompt(
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
    async fn put_prompt_rejects_missing_or_invalid_provenance_note() {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(write_inspection(false)));
        storage.expect_store_secret().times(0);
        let mut process = ports::MockSecretInputPort::new();
        process
            .expect_read_bws_access_token_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| {
                Ok(bws_token(
                    "7325b357-a802-49eb-a65f-a8b94ff65b2d",
                    "candidateSecret123",
                ))
            });
        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_ensure_recovery_token_provenance()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| {
                Err(anyhow::anyhow!(
                    "refusing to store bws-access-token: password-store-remote is missing provisioning token provenance"
                ))
            });

        let result = run_put_with_prompt(
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
