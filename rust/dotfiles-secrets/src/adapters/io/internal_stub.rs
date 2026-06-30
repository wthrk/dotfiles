//! `secrets-internal-test-stub` feature 専用の process I/O adapter stub。
//!
//! この module は production build へ compile されず、CLI integration test が同一 binary の
//! production command path を非対話で実行するためだけに入力 port を固定値へ接続する。
//! stdout observation は持たず、secret 値は feature 有効時の in-process stub 境界から外へ出さない。

use crate::{
    Result,
    domain::gpg_restore::OpenSshPublicKey,
    ports::io::{
        PasswordStoreRemoteInputPort, PinInputPort, SecretInputPort, SecretOutputPort,
        SshPublicKeyOutputPort,
    },
    support::protection::{ProtectedSecret, write_secret_stdout},
};

/// internal stub feature で process I/O port を非対話固定値へ翻訳する adapter。
#[derive(Default)]
pub(super) struct ProcessIoAdapter;

impl PinInputPort for ProcessIoAdapter {
    fn read_pin(&self) -> Result<ProtectedSecret> {
        ProtectedSecret::from_test_bytes(b"123456")
    }
}

impl SecretInputPort for ProcessIoAdapter {
    fn read_bitwarden_client_id_secret(&self) -> Result<ProtectedSecret> {
        ProtectedSecret::from_test_bytes(b"stub-client-id")
    }

    fn read_bitwarden_client_secret(&self) -> Result<ProtectedSecret> {
        ProtectedSecret::from_test_bytes(b"stub-client-secret")
    }

    fn read_bitwarden_master_password(&self) -> Result<ProtectedSecret> {
        ProtectedSecret::from_test_bytes(b"stub-master-password")
    }
}

impl PasswordStoreRemoteInputPort for ProcessIoAdapter {
    fn read_password_store_remote_url(&self) -> Result<String> {
        Ok("git@github.com:example-owner/password-store.git".to_owned())
    }
}

impl SecretOutputPort for ProcessIoAdapter {
    fn write_secret(&self, secret: &ProtectedSecret) -> Result<()> {
        write_secret_stdout(secret)
    }
}

impl SshPublicKeyOutputPort for ProcessIoAdapter {
    fn write_ssh_public_key(&self, public_key: &OpenSshPublicKey) -> Result<()> {
        println!("{}", public_key.as_str());
        Ok(())
    }
}
