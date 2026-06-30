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
        gpg_backup::{EnvelopeCiphertext, PrimaryFingerprint, SecretPrimaryKeyCandidates},
        gpg_restore::{ImportedKeyComposition, Keygrip, OpenSshPublicKey, SshAgentReadiness},
        pass_restore::GpgRecipientId,
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
    /// ローカル鍵リング上の使用可能な GPG secret primary key 候補を列挙する。
    ///
    /// implementor は revoked / expired / disabled / public-only など外部 API 上の状態を境界型へ翻訳する。
    /// 0 件 / 1 件 / 複数件の停止条件や、`.gpg-id` など既存設定を優先するかどうかは caller 側で決める。
    fn list_secret_primary_fingerprints(&mut self) -> Result<SecretPrimaryKeyCandidates>;

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

    /// import 後の検証失敗時に、不完全な状態で取り込んだ secret key を鍵リングから削除する。
    ///
    /// import 直後の subkey 検証が失敗した場合のロールバックに使い、不完全鍵を残して次回 restore を
    /// 衝突で復旧不能にしないための best-effort 削除を担う。
    fn delete_secret_key(&mut self, primary_fingerprint: &PrimaryFingerprint) -> Result<()>;

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

    /// `.gpg-id` recipient 宛ての復号に使える秘密鍵を鍵リングが保持しているかを確認する。
    ///
    /// `pass` は `.gpg-id` recipient の公開鍵で各 entry を暗号化する。その recipient に対応する秘密鍵を
    /// 手元に持たない場合、clone は成功しても `pass` は復号できない。implementor は recipient（long key id /
    /// fingerprint）で秘密鍵を解決できるかだけを返し、復号可否の最終判定は caller（application）が行う。
    fn secret_key_available_for_recipient(&mut self, recipient: &GpgRecipientId) -> Result<bool>;

    /// `.gpg-id` recipient が解決する secret primary fingerprint を返す。
    ///
    /// implementor は recipient token を鍵リング API へ渡し、見つかった secret key の primary fingerprint を
    /// 境界型へ翻訳するだけにする。複数 recipient が同じ primary を指すか、複数 primary へ分かれて曖昧かは
    /// caller/domain 側で判定する。
    fn primary_fingerprint_for_recipient(
        &mut self,
        recipient: &GpgRecipientId,
    ) -> Result<Option<PrimaryFingerprint>>;

    /// store 内サンプル entry（`*.gpg`）を gpgme で復号できることを確認する。
    ///
    /// `.gpg-id` recipient と手元秘密鍵の整合だけでなく、実際に store entry を復号できることまで確認する
    /// ための capability である。entry が暗号化された `pass` 形式であり、復元済み秘密鍵で復号できれば成功する。
    /// 復号した平文は保護境界内で破棄し、argv / log / 永続ファイル・stdout へ出さない。
    fn can_decrypt_store_entry(&mut self, entry_path: &std::path::Path) -> Result<()>;
}

/// use case が backup envelope の DEK 暗復号のために要求する capability 契約。
///
/// caller は recipient 照合と unwrap 後の復号順序を application/domain 側で決める。
/// implementor は DEK での envelope 本体 decrypt を `support/protection` 境界へ翻訳するだけで、
/// envelope schema や recipient 照合の業務規則は持たない。
/// DEK と復号済み backup は `ProtectedSecret` の保護境界内で扱う。
#[cfg_attr(test, mockall::automock)]
pub trait BackupCipherPort {
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
/// gpg-agent の SSH key list（`sshcontrol` 相当）登録と SSH agent socket 上の identity 列挙観測だけを担い、
/// `gpgconf` CLI は使わず `${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` を優先候補として解決する。
#[cfg_attr(test, mockall::automock)]
pub trait SshAgentPort {
    /// authentication subkey の keygrip を gpg-agent の SSH key list へ登録する（既登録は冪等）。
    fn register_authentication_subkey(&mut self, keygrip: &Keygrip) -> Result<()>;

    /// gpg-agent SSH support 利用可否と、agent が復元鍵の authentication subkey identity を識別可能かを観測する。
    ///
    /// agent が SSH agent protocol で列挙する identity を取得し、socket 解決可否（`socket_resolved`）に加えて、
    /// 期待公開鍵（`authentication_subkey_ssh_public_key` 由来の `OpenSshPublicKey`）と key blob が byte 一致する
    /// identity が含まれるか（`recovery_identity_present`）を `SshAgentReadiness` へ翻訳する。caller は復元鍵提示の
    /// 確認を `SshAgentReadiness::ensure_ready` で行う。復元鍵と無関係な既存 identity の有無は
    /// 観測しない。identity comment（`cardno:` / `openpgp:` 等）は鍵同一性に使えないため照合に用いない。
    fn inspect_ssh_agent(
        &mut self,
        expected_public_key: &OpenSshPublicKey,
    ) -> Result<SshAgentReadiness>;
}
