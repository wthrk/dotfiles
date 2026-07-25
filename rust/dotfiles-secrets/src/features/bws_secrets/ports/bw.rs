//! Bitwarden Secrets Manager backend へ application が要求する port 契約。
//!
//! この module は BWS の project/secret 候補取得と secret value 取得の capability だけを宣言し、
//! SDK 認証や UUID 型変換の詳細を adapter 側へ閉じる。

use crate::{
    Result,
    features::{
        gpg_backup_recovery::ports::public::{BackupUpdateGuard, GpgBackupEnvelope},
        password_store::ports::public::PasswordStoreRemote,
    },
    foundation::protection::ProtectedSecret,
};

use super::public::{BwsLookupCandidate, BwsProjectId, BwsSecretId};

/// use case が Bitwarden Secrets Manager API 境界へ要求する契約。
///
/// caller は domain lookup rule と外部確認 plan を application/domain 側で適用する。implementor は
/// SDK 認証、project/secret 候補の外部 API 取得、ID 境界変換、返却 secret の保護値化だけを担い、
/// 平文 token や secret value を application へ返さない。
#[cfg_attr(test, mockall::automock)]
pub trait BwsClientPort {
    async fn list_bws_projects(
        &self,
        access_token: &ProtectedSecret,
    ) -> Result<Vec<BwsLookupCandidate<BwsProjectId>>>;

    async fn list_bws_secrets(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
    ) -> Result<Vec<BwsLookupCandidate<BwsSecretId>>>;

    /// 解決済み BWS secret を検証済み GPG backup envelope と stale-overwrite 防止 guard として取得する。
    ///
    /// implementor は SDK の get、revision / value bytes からの guard 構築、保護境界内での UTF-8
    /// validation を担う。envelope の wire-format 検証は [`GpgBackupEnvelope::from_json`] という
    /// domain contract に従い、SDK response の `String` と `ProtectedSecret` を application へ返さない。
    /// `gpg-secret-key-backup` の value は encrypted envelope であり平文鍵素材を含まない。
    async fn fetch_gpg_backup_envelope(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> Result<(GpgBackupEnvelope, BackupUpdateGuard)>;

    /// 解決済み `password-store-remote` を検証済み domain 値として取得する。
    ///
    /// implementor は SDK get と保護境界内の UTF-8 validation を担い、GitHub SSH clone URL の検証は
    /// [`PasswordStoreRemote::parse`] という domain contract に従う。clone URL は credential ではないが
    /// private repository の所在を示すため、caller は表示・log・report に出さない。
    async fn fetch_password_store_remote(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> Result<PasswordStoreRemote>;

    /// 指定 project に新しい `gpg-secret-key-backup` envelope を作成し、その ID を返す。
    ///
    /// 実装は envelope を canonical JSON へ serialize して SDK の create 境界へ翻訳するだけで、登録対象の
    /// 同一性判断や上書き可否の業務判断は持たない。serialize 結果は暗号化済み envelope であり平文鍵素材を
    /// 含まない。`secret_key` は application/domain が決めた対象同一性であり、backend は選択・再解釈しない。
    async fn create_gpg_backup_envelope(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
        secret_key: &str,
        envelope: &GpgBackupEnvelope,
    ) -> Result<BwsSecretId>;

    /// stale overwrite 防止 guard が現行値と一致する場合だけ、既存 envelope を新しい envelope へ更新する。
    ///
    /// implementor は更新直前に現行値を再取得し、その guard が `expected_guard` と一致する場合だけ SDK の
    /// update 境界へ進む。一致しなければ stale overwrite として停止する。`version` と
    /// `metadata.primary_fingerprint` だけを判定条件にしてはならない。
    async fn update_gpg_backup_envelope_if_unchanged(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
        secret_id: &BwsSecretId,
        secret_key: &str,
        envelope: &GpgBackupEnvelope,
        expected_guard: &BackupUpdateGuard,
    ) -> Result<()>;

    /// 指定 project に新しい `password-store-remote` secret を作成し、その ID を返す。
    ///
    /// `remote` は application が `--url` または可視プロンプト/pipe 入力を domain rule
    /// [`PasswordStoreRemote::parse`] で検証した値である。clone URL は秘密情報ではないため `ProtectedSecret`
    /// ではなく検証済み domain 値で運ぶ。implementor は検証済み URL 文字列を SDK の create 境界へ翻訳する
    /// だけで、URL 形式の再検証や保護 buffer 化を行わない。`secret_key` は application/domain が決めた
    /// 対象同一性であり、backend は選択・再解釈しない。
    async fn create_password_store_remote(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
        secret_key: &str,
        remote: &PasswordStoreRemote,
    ) -> Result<BwsSecretId>;

    /// `password-store-remote` の更新直前確認に使う stale overwrite 防止 guard を取得する。
    ///
    /// implementor は現行 secret の SDK revision / updatedAt / ETag 相当を
    /// [`BackupUpdateGuard::from_revision`] で、取得できなければ exact value bytes から
    /// [`BackupUpdateGuard::from_value_bytes`] で fallback guard を作る。secret value 本体は
    /// application へ返さず、guard だけを返す。
    async fn fetch_password_store_remote_guard(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> Result<BackupUpdateGuard>;

    /// stale overwrite 防止 guard が現行値と一致する場合だけ、既存 `password-store-remote` を新値へ更新する。
    ///
    /// implementor は更新直前に現行値を再取得し、その guard が `expected_guard` と一致する場合だけ SDK の
    /// update 境界へ進む。一致しなければ stale overwrite として停止する。`remote` は application が domain rule
    /// [`PasswordStoreRemote::parse`] で検証済みの非秘匿 clone URL であり、implementor は再検証しない。
    async fn update_password_store_remote_if_unchanged(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
        secret_id: &BwsSecretId,
        secret_key: &str,
        remote: &PasswordStoreRemote,
        expected_guard: &BackupUpdateGuard,
    ) -> Result<()>;
}
