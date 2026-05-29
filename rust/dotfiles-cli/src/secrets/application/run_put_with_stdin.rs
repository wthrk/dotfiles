//! put(stdin) の順序責務を保持し、stdin 読み取り実装と storage 実装の境界を固定する。

use crate::Result;
use crate::secrets::{
    domain::{command::PutCommand, storage::SecretStorageWriteIntent},
    ports,
};

/// 非対話 stdin から受け取った secret を対象 serial の YubiKey storage へ保存する。
///
/// use case は入力取得と保存順序のみを担い、stdin 条件やサイズ制約は adapter 実装側へ閉じ込める。
pub(crate) fn run_put_with_stdin<P, S>(
    command: PutCommand,
    process: &P,
    storage_port: &mut S,
) -> Result<()>
where
    P: ports::SecretInputPort,
    S: ports::SecretStoragePort,
{
    let serial = command.required_serial()?;
    let storage = command.storage_spec(serial);
    let inspection = storage_port.inspect_secret_storage_write(serial, &storage)?;
    SecretStorageWriteIntent::ensure_put_preconditions(&storage, &inspection, command.force)?;
    let secret = process.read_streamed_secret()?;
    let intent = SecretStorageWriteIntent::put(storage, inspection, command.force, secret.len())?;
    storage_port.store_secret(serial, intent, &secret)
}

#[cfg(test)]
mod tests {
    use crate::secrets::{
        domain::{
            command::PutCommand, manifest::SecretManifest, piv::SecretName,
            storage::SecretStorageWriteInspection,
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_put_with_stdin;

    fn write_inspection(object_exists: bool) -> SecretStorageWriteInspection {
        SecretStorageWriteInspection {
            manifest_bytes: Some(SecretManifest::expected().encode().expect("manifest")),
            object_exists,
        }
    }

    #[test]
    fn put_stdin_checks_storage_before_reading_secret() {
        let process = ports::MockSecretInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        let mut sequence = mockall::Sequence::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(write_inspection(true)));
        storage.expect_store_secret().times(0);

        let result = run_put_with_stdin(
            PutCommand {
                serial: Some(2001),
                name: SecretName::BwsAccessToken,
                force: false,
            },
            &process,
            &mut storage,
        );

        assert!(
            result.is_err(),
            "preflight failure must stop before stdin read"
        );
    }

    #[test]
    fn put_stdin_stores_requested_secret() -> crate::Result<()> {
        let mut process = ports::MockSecretInputPort::new();
        process
            .expect_read_streamed_secret()
            .times(1)
            .returning(|| Ok(ProtectedSecret::from_test_bytes(b"token").expect("test secret")));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(1)
            .returning(|_, _| Ok(write_inspection(false)));
        storage
            .expect_store_secret()
            .times(1)
            .withf(|serial, intent, _| {
                *serial == 2001 && intent.storage.name == SecretName::BwsAccessToken
            })
            .returning(|_, _, _| Ok(()));

        run_put_with_stdin(
            PutCommand {
                serial: Some(2001),
                name: SecretName::BwsAccessToken,
                force: false,
            },
            &process,
            &mut storage,
        )
    }
}
