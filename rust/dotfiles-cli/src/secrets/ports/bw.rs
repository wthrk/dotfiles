//! Bitwarden Secrets Manager backend へ application が要求する port 契約。
//!
//! この module は BWS の project/secret 候補取得と secret value 取得の capability だけを宣言し、
//! SDK 認証や UUID 型変換の詳細を adapter 側へ閉じる。

use super::super::{
    domain::{
        bw_login::{BwLoginEmail, BwOtp, BwSessionKey},
        bws::{BwsLookupCandidate, BwsProjectId, BwsProjectName, BwsSecretId},
        gpg_backup::GpgBackupEnvelope,
        pass_restore::PasswordStoreRemote,
    },
    support::protection::ProtectedSecret,
};
use crate::Result;

/// use case が Bitwarden Password Manager CLI（`bw`）の login / unlock 境界へ要求する契約。
///
/// `bw` CLI の用途は login / unlock に限る（spec L84 / L192）。caller（application）は YubiKey 由来 secret の
/// 取得順序と email override 判断を持ち、検証済みの login email / OTP と保護値 master password を渡すだけにする。
/// implementor は `bw login <email> --passwordenv BW_PASSWORD --method 3 --code <otp>` と
/// `bw unlock --passwordenv BW_PASSWORD --raw` を子プロセスとして実行し、master password を子プロセスの
/// `BW_PASSWORD` env でだけ渡す。master password を argv / ログ / shell history / 一時ファイル / 親プロセスの
/// 永続環境変数へ残してはならない。返値は `bw unlock --raw` が stdout に出した session key（`BW_SESSION` 値）で、
/// disk / dotfile へ永続化しない。
#[cfg_attr(test, mockall::automock)]
pub trait BwLoginPort {
    async fn login_and_unlock(
        &self,
        email: &BwLoginEmail,
        password: &ProtectedSecret,
        otp: &BwOtp,
    ) -> Result<BwSessionKey>;
}

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

    /// 復旧用 BWS project が未作成の場合に新規作成し、その ID を返す。
    ///
    /// caller は project 名の固定値と 0件/1件/複数件の判断を domain/application 側で済ませる。
    /// implementor は検証済み project name を SDK create 境界へ翻訳するだけで、既存判定や重複解決の
    /// 業務判断を持たない。
    async fn create_bws_project(
        &self,
        access_token: &ProtectedSecret,
        project_name: BwsProjectName,
    ) -> Result<BwsProjectId>;

    async fn list_bws_secrets(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
    ) -> Result<Vec<BwsLookupCandidate<BwsSecretId>>>;

    /// `gpg-secret-key-backup` の encrypted envelope を取得する。
    ///
    /// implementor は取得した secret value bytes を [`GpgBackupEnvelope::from_json`] で domain 値へ
    /// 翻訳する。secret 値は encrypted envelope であり平文鍵素材を含まない。
    async fn fetch_gpg_backup_envelope(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> Result<GpgBackupEnvelope>;

    /// `password-store-remote` secret value を取得し、GitHub SSH clone URL として domain 検証した値を返す。
    ///
    /// implementor は取得した secret value 文字列を [`PasswordStoreRemote::parse`] で domain 値へ翻訳する。
    /// clone URL は秘密情報ではないため `ProtectedSecret` ではなく検証済み domain 値として返し、URL 形式の
    /// 妥当性判断（`git@github.com:<owner>/<repo>.git`）は domain rule に委ねて adapter で再定義しない。
    async fn fetch_password_store_remote(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> Result<PasswordStoreRemote>;

    /// 候補 `bws-access-token` が provisioning 用 token の再利用ではないことを確認する。
    ///
    /// caller は token 値そのものや opaque token id を扱わず、候補 token を渡して provenance gate の
    /// 合否だけを受け取る。implementor は candidate token から必要な非機密 id を抽出し、
    /// `password-store-remote` note の provenance marker 取得までを技術境界内で完了する。
    async fn ensure_recovery_token_provenance(&self, access_token: &ProtectedSecret) -> Result<()>;

    /// 指定 project に新しい `password-store-remote` secret を作成し、その ID を返す。
    ///
    /// `remote` は application が input port から取得し、domain rule [`PasswordStoreRemote::parse`] で
    /// 検証した値である。clone URL は秘密情報ではないため `ProtectedSecret` ではなく検証済み domain 値で
    /// 運ぶ。implementor は検証済み URL 文字列を SDK の create 境界へ翻訳するだけで、URL 形式の再検証や
    /// 保護 buffer 化を行わない。typed capability は常に `password-store-remote` を対象にし、caller は
    /// secret key 文字列を渡さない。
    async fn create_password_store_remote(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
        remote: &PasswordStoreRemote,
    ) -> Result<BwsSecretId>;
}
