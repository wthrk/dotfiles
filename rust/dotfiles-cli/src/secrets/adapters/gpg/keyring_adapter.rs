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

    fn delete_secret_key(&mut self, primary_fingerprint: &PrimaryFingerprint) -> Result<()> {
        let mut context = Self::context()?;
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
        let mut context = Self::context()?;
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
        let mut context = Self::context()?;
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
        let mut context = Self::context()?;
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
}

/// import 後鍵から「登録・公開鍵出力の対象とする」authentication subkey を単一の述語で特定する。
///
/// keygrip 解決と OpenSSH 公開鍵 export が同一 subkey を選ぶよう、両者はこの選択結果を共有する。選択述語は
/// 「primary key でない」「authentication 能力を持つ」「secret material を保持する」「revoked/expired/disabled
/// でない」を満たす最初の subkey とする。`subkeys()` の列挙順を唯一の決定基準として両経路で一致させるため、
/// 同一基準の `find` を 1 箇所に集約する。複数の有効 authentication subkey がある場合でも、両経路が同じ
/// subkey（列挙順で最初の有効鍵）を指すことを保証する。
fn select_authentication_subkey<'key>(key: &'key gpgme::Key) -> Result<gpgme::Subkey<'key>> {
    let primary_fp = key.fingerprint().ok().map(str::to_owned);
    key.subkeys()
        .filter(|subkey| !is_primary_subkey(subkey, primary_fp.as_deref()))
        .find(|subkey| {
            subkey.can_authenticate()
                && subkey.is_secret()
                && !subkey.is_revoked()
                && !subkey.is_expired()
                && !subkey.is_disabled()
        })
        .context("GPG authentication subkey could not be resolved")
}

/// gpgme の `subkeys()` 列挙要素が primary key かどうかを fingerprint 一致で判定する。
///
/// `subkeys()` は先頭に primary key を含むため、subkey 構成検証や authentication subkey 解決で
/// primary を subkey と数えないようにこの判定で除外する。fingerprint は大文字小文字を無視して照合する。
fn is_primary_subkey(subkey: &gpgme::Subkey<'_>, primary_fingerprint: Option<&str>) -> bool {
    match (subkey.fingerprint().ok(), primary_fingerprint) {
        (Some(subkey_fp), Some(primary_fp)) => subkey_fp.eq_ignore_ascii_case(primary_fp),
        _ => false,
    }
}
