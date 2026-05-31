//! `GpgKeyringPort` を gpgme（鍵リング metadata I/O）と `support/protection` の secret-key backend
//! 操作へ接続する adapter。
//!
//! secret key material の平文 borrow を伴う export / import / OpenPGP 解析は `support/protection` の
//! 専用 backend 操作（`gpg_backup`）に閉じ、この adapter は fingerprint・subkey capability・keygrip・
//! OpenSSH 公開鍵という非 secret metadata を gpgme で読み、domain 値（`ImportedKeyComposition` /
//! `Keygrip` / `OpenSshPublicKey` / `*Fingerprint`）へ翻訳する。既存鍵衝突判定や subkey 利用可能判定
//! そのものの業務規則は domain へ残す。`gpg` CLI は使わない。

use anyhow::Context;
use gpgme::{ExportMode, Protocol};

use crate::{
    Result,
    secrets::{
        domain::{
            gpg_backup::PrimaryFingerprint,
            gpg_restore::{
                ImportedKeyComposition, Keygrip, OpenSshPublicKey, ResolvedSubkey, SubkeyCapability,
            },
        },
        ports::gpg::GpgKeyringPort,
        support::protection::{ProtectedSecret, gpg_backup as backup_protection},
    },
};

/// gpgme + `support/protection` の secret-key backend を `GpgKeyringPort` 契約へ翻訳する adapter。
#[derive(Default)]
pub(super) struct GpgKeyringAdapter;

impl GpgKeyringAdapter {
    /// OpenPGP protocol の gpgme context を生成する（非 secret metadata 取得用）。
    fn context() -> Result<gpgme::Context> {
        gpgme::Context::from_protocol(Protocol::OpenPgp).context("failed to create gpgme context")
    }
}

impl GpgKeyringPort for GpgKeyringAdapter {
    fn export_secret_key(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<ProtectedSecret> {
        backup_protection::export_secret_key(primary_fingerprint.as_str())
    }

    fn parse_backup_primary_fingerprint(
        &mut self,
        backup: &ProtectedSecret,
    ) -> Result<PrimaryFingerprint> {
        let hex = backup_protection::parse_primary_fingerprint_hex(backup)?;
        PrimaryFingerprint::parse(&hex)
    }

    fn secret_key_exists(&mut self, primary_fingerprint: &PrimaryFingerprint) -> Result<bool> {
        let mut context = Self::context()?;
        match context.get_secret_key(primary_fingerprint.as_str()) {
            Ok(_) => Ok(true),
            Err(error)
                if error.code() == gpgme::Error::NO_SECKEY.code()
                    || error.code() == gpgme::Error::EOF.code() =>
            {
                Ok(false)
            }
            Err(error) => Err(anyhow::Error::new(error).context("failed to query GPG secret key")),
        }
    }

    fn import_secret_key(&mut self, backup: &ProtectedSecret) -> Result<PrimaryFingerprint> {
        let hex = backup_protection::import_secret_key(backup)?;
        PrimaryFingerprint::parse(&hex)
    }

    fn inspect_imported_key(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<ImportedKeyComposition> {
        let mut context = Self::context()?;
        let key = context
            .get_secret_key(primary_fingerprint.as_str())
            .context("failed to resolve imported GPG key")?;
        let mut subkeys = Vec::new();
        for subkey in key.subkeys() {
            let usable = !subkey.is_revoked() && !subkey.is_expired() && !subkey.is_disabled();
            if subkey.can_encrypt() {
                subkeys.push(ResolvedSubkey {
                    capability: SubkeyCapability::Encryption,
                    usable,
                });
            }
            if subkey.can_authenticate() {
                subkeys.push(ResolvedSubkey {
                    capability: SubkeyCapability::Authentication,
                    usable,
                });
            }
            if subkey.can_sign() {
                subkeys.push(ResolvedSubkey {
                    capability: SubkeyCapability::Signing,
                    usable,
                });
            }
        }
        Ok(ImportedKeyComposition::new(key.has_secret(), subkeys))
    }

    fn authentication_subkey_keygrip(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<Keygrip> {
        let mut context = Self::context()?;
        let key = context
            .get_secret_key(primary_fingerprint.as_str())
            .context("failed to resolve imported GPG key")?;
        let keygrip = key
            .subkeys()
            .find(|subkey| {
                subkey.can_authenticate()
                    && !subkey.is_revoked()
                    && !subkey.is_expired()
                    && !subkey.is_disabled()
            })
            .and_then(|subkey| subkey.keygrip().ok().map(str::to_owned))
            .context("GPG authentication subkey keygrip could not be resolved")?;
        Keygrip::parse(&keygrip)
    }

    fn authentication_subkey_ssh_public_key(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<OpenSshPublicKey> {
        let mut context = Self::context()?;
        let mut data = gpgme::Data::new().context("failed to allocate gpgme ssh export buffer")?;
        context
            .export([primary_fingerprint.as_str()], ExportMode::SSH, &mut data)
            .context("failed to export GPG authentication subkey as OpenSSH public key")?;
        let bytes = data
            .try_into_bytes()
            .context("failed to read exported OpenSSH public key bytes")?;
        let text =
            String::from_utf8(bytes).context("exported OpenSSH public key is not valid UTF-8")?;
        // SSH export は `<type> <base64> [comment]\n` を返す。先頭の非空行だけを domain 検証へ渡す。
        let line = text
            .lines()
            .find(|line| !line.trim().is_empty())
            .context("exported OpenSSH public key is empty")?;
        OpenSshPublicKey::parse(line)
    }
}
