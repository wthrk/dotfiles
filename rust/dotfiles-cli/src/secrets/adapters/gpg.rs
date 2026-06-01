//! GPG 鍵リング / OpenPGP 解析 / backup envelope DEK 暗復号 / gpg-agent SSH support を
//! port 契約へ接続する adapter 群。
//!
//! production build（`gpg-backend`）では gpgme + sequoia-openpgp + `support/protection` の secret-key
//! backend と gpg-agent sshcontrol/socket 観測へ接続する。`secrets-internal-test-stub` feature では
//! 同じ port 契約を満たす internal backend stub と compile-time で差し替え、runtime real/stub 分岐は作らない。
//! integration test は stub module を import せず、feature 有効でビルドされた同じ `dotfiles` binary を実行する。

#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
mod cipher_adapter;
#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
mod keyring_adapter;
#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
mod ssh_agent_adapter;

#[cfg(feature = "secrets-internal-test-stub")]
mod internal_stub;

use crate::{
    Result,
    secrets::{
        domain::{
            gpg_backup::{EnvelopeCiphertext, PrimaryFingerprint},
            gpg_restore::{ImportedKeyComposition, Keygrip, OpenSshPublicKey, SshAgentReadiness},
        },
        ports::gpg::{BackupCipherPort, GpgKeyringPort, SshAgentPort},
        support::protection::ProtectedSecret,
    },
};

/// GPG 鍵リング backend（gpgme + sequoia / internal stub）を `GpgKeyringPort` 契約へ翻訳する adapter。
#[derive(Default)]
pub(in crate::secrets) struct GpgKeyringAdapter(GpgKeyringInner);

#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
type GpgKeyringInner = keyring_adapter::GpgKeyringAdapter;
#[cfg(feature = "secrets-internal-test-stub")]
type GpgKeyringInner = internal_stub::GpgKeyringStub;

impl GpgKeyringPort for GpgKeyringAdapter {
    fn export_secret_key(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<ProtectedSecret> {
        self.0.export_secret_key(primary_fingerprint)
    }

    fn parse_backup_primary_fingerprint(
        &mut self,
        backup: &ProtectedSecret,
    ) -> Result<PrimaryFingerprint> {
        self.0.parse_backup_primary_fingerprint(backup)
    }

    fn secret_key_exists(&mut self, primary_fingerprint: &PrimaryFingerprint) -> Result<bool> {
        self.0.secret_key_exists(primary_fingerprint)
    }

    fn import_secret_key(&mut self, backup: &ProtectedSecret) -> Result<PrimaryFingerprint> {
        self.0.import_secret_key(backup)
    }

    fn delete_secret_key(&mut self, primary_fingerprint: &PrimaryFingerprint) -> Result<()> {
        self.0.delete_secret_key(primary_fingerprint)
    }

    fn inspect_imported_key(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<ImportedKeyComposition> {
        self.0.inspect_imported_key(primary_fingerprint)
    }

    fn authentication_subkey_keygrip(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<Keygrip> {
        self.0.authentication_subkey_keygrip(primary_fingerprint)
    }

    fn authentication_subkey_ssh_public_key(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<OpenSshPublicKey> {
        self.0
            .authentication_subkey_ssh_public_key(primary_fingerprint)
    }
}

/// backup envelope の DEK 暗復号 backend を `BackupCipherPort` 契約へ翻訳する adapter。
#[derive(Default)]
pub(in crate::secrets) struct BackupCipherAdapter(BackupCipherInner);

#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
type BackupCipherInner = cipher_adapter::BackupCipherAdapter;
#[cfg(feature = "secrets-internal-test-stub")]
type BackupCipherInner = internal_stub::BackupCipherStub;

impl BackupCipherPort for BackupCipherAdapter {
    fn generate_dek(&mut self) -> Result<ProtectedSecret> {
        self.0.generate_dek()
    }

    fn encrypt_backup(
        &mut self,
        dek: &ProtectedSecret,
        backup: &ProtectedSecret,
    ) -> Result<EnvelopeCiphertext> {
        self.0.encrypt_backup(dek, backup)
    }

    fn decrypt_backup(
        &mut self,
        dek: &ProtectedSecret,
        ciphertext: &EnvelopeCiphertext,
    ) -> Result<ProtectedSecret> {
        self.0.decrypt_backup(dek, ciphertext)
    }
}

/// gpg-agent SSH support backend を `SshAgentPort` 契約へ翻訳する adapter。
#[derive(Default)]
pub(in crate::secrets) struct SshAgentAdapter(SshAgentInner);

#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
type SshAgentInner = ssh_agent_adapter::SshAgentAdapter;
#[cfg(feature = "secrets-internal-test-stub")]
type SshAgentInner = internal_stub::SshAgentStub;

impl SshAgentPort for SshAgentAdapter {
    fn register_authentication_subkey(&mut self, keygrip: &Keygrip) -> Result<()> {
        self.0.register_authentication_subkey(keygrip)
    }

    fn inspect_ssh_agent(
        &mut self,
        expected_public_key: &OpenSshPublicKey,
    ) -> Result<SshAgentReadiness> {
        self.0.inspect_ssh_agent(expected_public_key)
    }
}
