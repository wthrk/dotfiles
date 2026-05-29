//! Bitwarden Secrets Manager backend capability の port 契約。

use crate::Result;
use crate::secrets::domain::values::{BwsLookupCandidate, BwsProjectId, BwsSecretId};
use crate::secrets::support::protection::ProtectedSecret;

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
