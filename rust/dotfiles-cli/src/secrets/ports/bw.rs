//! Bitwarden Secrets Manager backend へ要求する port 契約。
//!
//! BWS project/secret 候補取得と secret value 取得だけを外部境界として宣言する。

use crate::{
    Result,
    secrets::{
        domain::bws::{BwsLookupCandidate, BwsProjectId, BwsSecretId},
        support::protection::ProtectedSecret,
    },
};

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

    async fn fetch_bws_secret_by_id(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> Result<ProtectedSecret>;
}
