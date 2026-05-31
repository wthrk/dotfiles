//! GPG 鍵リング / OpenPGP 解析 / gpg-agent SSH support backend へ application が要求する port 契約。
//!
//! この module は「既存環境からの secret key export」「encrypted backup の import」「import 後鍵の
//! subkey 構成の取得」「authentication subkey の keygrip / OpenSSH 公開鍵の解決」「gpg-agent の
//! SSH key list 登録」「gpg-agent SSH support 観測」という capability だけを宣言する。gpgme / sequoia
//! / gpg-agent socket の実装詳細、`gpg` CLI 呼び出し、PIV device API は adapter 側へ閉じ、ここには
//! 露出しない。secret key material は `ProtectedSecret` を carrier として受け渡し、平文取り出し API は
//! port へ持ち込まない。

use super::super::{
    domain::{
        gpg_backup::{EnvelopeCiphertext, PrimaryFingerprint},
        gpg_restore::{ImportedKeyComposition, Keygrip, OpenSshPublicKey, SshAgentReadiness},
    },
    support::protection::ProtectedSecret,
};
use crate::Result;

/// use case が GPG 鍵リング backend（gpgme + OpenPGP 解析）へ要求する capability 契約。
///
/// caller は backup export / import / subkey 検証の順序と停止条件を application/domain 側で決める。
/// implementor は gpgme context・OpenPGP transferable secret key・keygrip / ssh export の翻訳だけを担い、
/// 既存鍵衝突判定や subkey 利用可能判定そのものの業務規則は再定義しない。`gpg` CLI は使わない。
#[cfg_attr(test, mockall::automock)]
pub trait GpgKeyringPort {
    /// 既存環境のローカル鍵リングから、指定 primary fingerprint の OpenPGP transferable secret key を
    /// in-memory で export し、保護値として返す。secret material は argv / log / 永続ファイルへ出さない。
    fn export_secret_key(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<ProtectedSecret>;

    /// 復号済み backup bytes を OpenPGP として解析し、import 前に canonical primary fingerprint を導出する。
    ///
    /// gpgme へ渡す前のインメモリ解析で、envelope metadata との一致照合に使う fingerprint を確定する。
    fn parse_backup_primary_fingerprint(
        &mut self,
        backup: &ProtectedSecret,
    ) -> Result<PrimaryFingerprint>;

    /// 既存の鍵リングに、同一 primary fingerprint の secret key が存在するかを確認する。
    fn secret_key_exists(&mut self, primary_fingerprint: &PrimaryFingerprint) -> Result<bool>;

    /// 復号済み backup bytes を gpgme で鍵リングへ import し、import 結果から対象 primary fingerprint を返す。
    fn import_secret_key(&mut self, backup: &ProtectedSecret) -> Result<PrimaryFingerprint>;

    /// import 後の鍵を再取得し、subkey 構成（capability と利用可能状態）を domain 検証用に解決する。
    fn inspect_imported_key(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<ImportedKeyComposition>;

    /// import 後鍵の authentication subkey の keygrip を解決する。
    fn authentication_subkey_keygrip(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<Keygrip>;

    /// import 後鍵の authentication subkey 由来の OpenSSH 公開鍵 1 行を解決する。
    fn authentication_subkey_ssh_public_key(
        &mut self,
        primary_fingerprint: &PrimaryFingerprint,
    ) -> Result<OpenSshPublicKey>;
}

/// use case が backup envelope の DEK 暗復号のために要求する capability 契約。
///
/// caller は DEK の生成・recipient wrap・envelope 検証の順序を application/domain 側で決める。
/// implementor は AES-256-GCM の DEK 生成と、DEK での envelope 本体 encrypt / decrypt を
/// `support/protection` 境界へ翻訳するだけで、envelope schema や recipient 照合の業務規則は持たない。
/// DEK と復号済み backup は `ProtectedSecret` の保護境界内で扱う。
#[cfg_attr(test, mockall::automock)]
pub trait BackupCipherPort {
    /// AES-256-GCM の新しい DEK（32 bytes）を保護値として生成する。
    fn generate_dek(&mut self) -> Result<ProtectedSecret>;

    /// 平文 backup を DEK で AES-256-GCM 暗号化し、envelope `ciphertext`（nonce/body/tag）を返す。
    fn encrypt_backup(
        &mut self,
        dek: &ProtectedSecret,
        backup: &ProtectedSecret,
    ) -> Result<EnvelopeCiphertext>;

    /// envelope `ciphertext` を DEK で AES-256-GCM 復号し、復号済み backup を保護値として返す。
    fn decrypt_backup(
        &mut self,
        dek: &ProtectedSecret,
        ciphertext: &EnvelopeCiphertext,
    ) -> Result<ProtectedSecret>;
}

/// use case が gpg-agent の SSH support backend へ要求する capability 契約。
///
/// caller は keygrip の登録順序と SSH support 充足判定を application/domain 側で決める。implementor は
/// gpg-agent の SSH key list（`sshcontrol` 相当）登録と SSH agent socket 観測だけを担い、`gpgconf` CLI は
/// 使わず `${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` を優先候補として解決する。
#[cfg_attr(test, mockall::automock)]
pub trait SshAgentPort {
    /// authentication subkey の keygrip を gpg-agent の SSH key list へ登録する（既登録は冪等）。
    fn register_authentication_subkey(&mut self, keygrip: &Keygrip) -> Result<()>;

    /// gpg-agent SSH support 利用可否を、socket 解決可否と authentication subkey 識別可否として観測する。
    fn inspect_ssh_agent(&mut self, keygrip: &Keygrip) -> Result<SshAgentReadiness>;
}
