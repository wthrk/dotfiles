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
    domain::{
        gpg_backup::PrimaryFingerprint,
        gpg_restore::{
            ImportedKeyComposition, Keygrip, OpenSshPublicKey, ResolvedSubkey, SubkeyCapability,
        },
        pass_restore::GpgRecipientId,
    },
    ports::gpg::GpgKeyringPort,
    support::{
        adapter_backend::GpgKeyringBackend,
        gpg_keyring::{is_primary_subkey, select_authentication_subkey},
        protection::{ProtectedSecret, gpg_backup as backup_protection},
    },
};

impl GpgKeyringPort for GpgKeyringBackend {
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
        let mut context = gpgme::Context::from_protocol(Protocol::OpenPgp)
            .context("failed to create gpgme context")?;
        match context.get_secret_key(primary_fingerprint.as_str()) {
            Ok(_) => Ok(true),
            // GPGME Error Codes: https://gnupg.org/documentation/manuals/gpgme/Error-Codes.html
            // `GPG_ERR_NO_SECKEY` だけが secret key 不在を表す。`EOF` は key
            // lookup における「不在」として仕様化されていないため、状態へ変換せず伝播する。
            Err(error) if error.code() == gpgme::Error::NO_SECKEY.code() => Ok(false),
            Err(error) => Err(anyhow::Error::new(error).context("failed to query GPG secret key")),
        }
    }

    fn import_secret_key(&mut self, backup: &ProtectedSecret) -> Result<PrimaryFingerprint> {
        let hex = backup_protection::import_secret_key(backup)?;
        PrimaryFingerprint::parse(&hex)
    }

    fn delete_secret_key(&mut self, primary_fingerprint: &PrimaryFingerprint) -> Result<()> {
        let mut context = gpgme::Context::from_protocol(Protocol::OpenPgp)
            .context("failed to create gpgme context")?;
        let key = context
            .get_secret_key(primary_fingerprint.as_str())
            .context("failed to resolve GPG secret key for rollback deletion")?;
        context
            .delete_secret_key(&key)
            .context("failed to delete GPG secret key during rollback")
    }

    fn inspect_imported_key(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<ImportedKeyComposition> {
        let mut context = gpgme::Context::from_protocol(Protocol::OpenPgp)
            .context("failed to create gpgme context")?;
        let key = context
            .get_secret_key(primary_fingerprint.as_str())
            .context("failed to resolve imported GPG key")?;
        // gpgme の `subkeys()` は先頭に primary key を含む。primary が signing 能力を持つ場合に
        // signing subkey 不在でも検証通過してしまわないよう、primary fingerprint と一致する要素を
        // subkey 走査から除外する。
        let primary_fp = key.fingerprint().ok().map(str::to_owned);
        let mut subkeys = Vec::new();
        for subkey in key.subkeys() {
            if is_primary_subkey(&subkey, primary_fp.as_deref()) {
                continue;
            }
            // public-only subkey（secret material 不保持）は import 後も E/A/S を復元できないため、
            // revoked/expired/disabled に加えて `is_secret()` を usable 判定に含める。
            let usable = subkey.is_secret()
                && !subkey.is_revoked()
                && !subkey.is_expired()
                && !subkey.is_disabled();
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
        let mut context = gpgme::Context::from_protocol(Protocol::OpenPgp)
            .context("failed to create gpgme context")?;
        let key = context
            .get_secret_key(primary_fingerprint.as_str())
            .context("failed to resolve imported GPG key")?;
        let selected = select_authentication_subkey(&key)?;
        let keygrip = selected
            .keygrip()
            .ok()
            .context("GPG authentication subkey keygrip could not be resolved")?;
        Keygrip::parse(keygrip)
    }

    fn authentication_subkey_ssh_public_key(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<OpenSshPublicKey> {
        let mut context = gpgme::Context::from_protocol(Protocol::OpenPgp)
            .context("failed to create gpgme context")?;
        // keygrip 解決と同一の選択述語で authentication subkey を特定し、その subkey の fingerprint を
        // export pattern にする。fingerprint へ末尾 `!` を付けて「その subkey 自身」を export 対象に固定し、
        // gpgme/gpg が列挙順で別の authentication subkey を選んでしまう不一致を防ぐ（keygrip と公開鍵が
        // 同一 subkey を指すことを保証する）。
        let subkey_fingerprint = {
            let key = context
                .get_secret_key(primary_fingerprint.as_str())
                .context("failed to resolve imported GPG key")?;
            let selected = select_authentication_subkey(&key)?;
            selected
                .fingerprint()
                .ok()
                .context("GPG authentication subkey fingerprint could not be resolved")?
                .to_owned()
        };
        let mut data = gpgme::Data::new().context("failed to allocate gpgme ssh export buffer")?;
        context
            .export(
                [format!("{subkey_fingerprint}!")],
                ExportMode::SSH,
                &mut data,
            )
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

    fn secret_key_available_for_recipient(&mut self, recipient: &GpgRecipientId) -> Result<bool> {
        let mut context = gpgme::Context::from_protocol(Protocol::OpenPgp)
            .context("failed to create gpgme context")?;
        match context.get_secret_key(recipient.as_str()) {
            Ok(_) => Ok(true),
            // GPGME Error Codes: https://gnupg.org/documentation/manuals/gpgme/Error-Codes.html
            // `GPG_ERR_NO_SECKEY` だけが secret key 不在を表す。`EOF` は key
            // lookup における「不在」として仕様化されていないため、状態へ変換せず伝播する。
            Err(error) if error.code() == gpgme::Error::NO_SECKEY.code() => Ok(false),
            Err(error) => Err(anyhow::Error::new(error)
                .context("failed to query GPG secret key for password-store recipient")),
        }
    }

    fn can_decrypt_store_entry(&mut self, entry_path: &std::path::Path) -> Result<()> {
        // store 内サンプル entry を gpgme で復号し、復元済み秘密鍵で読めることを確認する。復号した平文は
        // この scope 内で破棄し、stdout / log / 一時ファイルへ出さない（保護境界内で完了させる）。
        backup_protection::verify_can_decrypt(entry_path)
    }
}
