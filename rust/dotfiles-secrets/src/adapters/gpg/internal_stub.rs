//! `secrets-internal-test-stub` feature の GPG port translation。

use anyhow::Context;

use crate::{
    Result,
    domain::{
        gpg_backup::{EnvelopeCiphertext, PrimaryFingerprint},
        gpg_restore::{
            ImportedKeyComposition, Keygrip, OpenSshPublicKey, ResolvedSubkey, SshAgentReadiness,
            SubkeyCapability,
        },
        pass_restore::GpgRecipientId,
    },
    ports::gpg::{BackupCipherPort, GpgKeyringPort, SshAgentPort},
    support::{
        adapter_backend::{BackupCipherBackend, GpgKeyringBackend, SshAgentBackend},
        internal_stub_gpg,
        protection::ProtectedSecret,
    },
};

impl GpgKeyringPort for GpgKeyringBackend {
    fn export_secret_key(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<ProtectedSecret> {
        ProtectedSecret::from_test_bytes(primary_fingerprint.as_str().as_bytes())
    }

    fn parse_backup_primary_fingerprint(
        &mut self,
        backup: &ProtectedSecret,
    ) -> Result<PrimaryFingerprint> {
        let fingerprint = String::from_utf8(backup.to_test_bytes())
            .context("internal gpg stub backup body is not valid UTF-8")?;
        PrimaryFingerprint::parse(fingerprint.trim())
    }

    fn secret_key_exists(&mut self, primary_fingerprint: &PrimaryFingerprint) -> Result<bool> {
        internal_stub_gpg::key_exists(primary_fingerprint.as_str())
    }

    fn import_secret_key(&mut self, backup: &ProtectedSecret) -> Result<PrimaryFingerprint> {
        let fingerprint = String::from_utf8(backup.to_test_bytes())
            .context("internal gpg stub backup body is not valid UTF-8")?;
        let fingerprint = PrimaryFingerprint::parse(fingerprint.trim())?;
        internal_stub_gpg::import_key(fingerprint.as_str())?;
        Ok(fingerprint)
    }

    fn delete_secret_key(&mut self, primary_fingerprint: &PrimaryFingerprint) -> Result<()> {
        internal_stub_gpg::delete_key(primary_fingerprint.as_str())
    }

    fn inspect_imported_key(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<ImportedKeyComposition> {
        let key = internal_stub_gpg::key_data(primary_fingerprint.as_str())?;
        Ok(ImportedKeyComposition::new(
            key.has_secret_material,
            key.capabilities
                .iter()
                .filter_map(|capability| match capability.as_str() {
                    "encryption" => Some(SubkeyCapability::Encryption),
                    "authentication" => Some(SubkeyCapability::Authentication),
                    "signing" => Some(SubkeyCapability::Signing),
                    _ => None,
                })
                .map(|capability| ResolvedSubkey {
                    capability,
                    usable: true,
                })
                .collect(),
        ))
    }

    fn authentication_subkey_keygrip(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<Keygrip> {
        Keygrip::parse(&internal_stub_gpg::key_data(primary_fingerprint.as_str())?.keygrip)
    }

    fn authentication_subkey_ssh_public_key(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<OpenSshPublicKey> {
        OpenSshPublicKey::parse(
            &internal_stub_gpg::key_data(primary_fingerprint.as_str())?.ssh_public_key,
        )
    }

    fn secret_key_available_for_recipient(&mut self, recipient: &GpgRecipientId) -> Result<bool> {
        internal_stub_gpg::held_recipient(recipient.as_str())
    }

    fn can_decrypt_store_entry(&mut self, _entry_path: &std::path::Path) -> Result<()> {
        internal_stub_gpg::ensure_store_entry_decryptable()
    }
}

impl BackupCipherPort for BackupCipherBackend {
    fn generate_dek(&mut self) -> Result<ProtectedSecret> {
        ProtectedSecret::from_test_bytes(&internal_stub_gpg::test_dek())
    }

    fn encrypt_backup(
        &mut self,
        _dek: &ProtectedSecret,
        backup: &ProtectedSecret,
    ) -> Result<EnvelopeCiphertext> {
        let (nonce, body, tag) = internal_stub_gpg::ciphertext_parts(backup.to_test_bytes());
        EnvelopeCiphertext::new(nonce, body, tag)
    }

    fn decrypt_backup(
        &mut self,
        _dek: &ProtectedSecret,
        ciphertext: &EnvelopeCiphertext,
    ) -> Result<ProtectedSecret> {
        ProtectedSecret::from_test_bytes(ciphertext.body())
    }
}

impl SshAgentPort for SshAgentBackend {
    fn register_authentication_subkey(&mut self, keygrip: &Keygrip) -> Result<()> {
        internal_stub_gpg::register_keygrip(keygrip.as_str())
    }

    fn inspect_ssh_agent(
        &mut self,
        expected_public_key: &OpenSshPublicKey,
    ) -> Result<SshAgentReadiness> {
        let recovery_identity_present = internal_stub_gpg::registered_ssh_public_keys()?
            .into_iter()
            .filter_map(|key| OpenSshPublicKey::parse(&key).ok())
            .filter_map(|key| key.key_blob())
            .any(|blob| expected_public_key.matches_agent_key_blob(&blob));
        Ok(SshAgentReadiness {
            socket_resolved: true,
            recovery_identity_present,
        })
    }
}
