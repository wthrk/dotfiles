//! `BwsClientPort` を Bitwarden Secrets Manager 取得境界へ接続する adapter。
//!
//! application は BWS lookup plan と domain の一意解決規則を保持する。adapter は SDK API の
//! project/secret/list/get 境界を port の ID 候補と保護済み secret へ翻訳する。

#[cfg(feature = "secrets-internal-test-stub")]
// `secrets-internal-test-stub` feature build だけで adapter 側 stub backend を接続する。
// production build には含めず、production command path は維持したまま、
// real/stub 切替は runtime 分岐ではなく compile-time selection。
use crate::secrets::adapters::stub;

#[cfg(not(feature = "secrets-internal-test-stub"))]
use bitwarden::secrets_manager::{
    projects::ProjectsListRequest, secrets::SecretIdentifiersByProjectRequest,
};
#[cfg(not(feature = "secrets-internal-test-stub"))]
use uuid::Uuid;

#[cfg(feature = "secrets-internal-test-stub")]
use crate::secrets::{
    domain::values::{BwsLookupCandidate, BwsProjectId, BwsSecretId},
    ports::bw::BwsClientPort,
    support::protection::ProtectedSecret,
};
#[cfg(not(feature = "secrets-internal-test-stub"))]
use crate::secrets::{
    domain::values::{BwsLookupCandidate, BwsProjectId, BwsSecretId},
    ports::bw::BwsClientPort,
    support::protection::{ProtectedSecret, bws},
};

/// Bitwarden Secrets Manager SDK を `BwsClientPort` へ翻訳する adapter。
#[derive(Default)]
pub(crate) struct BwsClientAdapter;

#[cfg(feature = "secrets-internal-test-stub")]
impl BwsClientPort for BwsClientAdapter {
    async fn list_bws_projects(
        &self,
        access_token: &ProtectedSecret,
    ) -> crate::Result<Vec<BwsLookupCandidate<BwsProjectId>>> {
        stub::bw::list_bws_projects(access_token)
    }

    async fn list_bws_secrets(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
    ) -> crate::Result<Vec<BwsLookupCandidate<BwsSecretId>>> {
        stub::bw::list_bws_secrets(access_token, project_id)
    }

    async fn fetch_bws_secret_by_id(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<ProtectedSecret> {
        stub::bw::fetch_bws_secret_by_id(access_token, secret_id)
    }
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
impl BwsClientPort for BwsClientAdapter {
    /// SDK project 一覧を port 境界の lookup 候補へ変換する。
    async fn list_bws_projects(
        &self,
        access_token: &ProtectedSecret,
    ) -> crate::Result<Vec<BwsLookupCandidate<BwsProjectId>>> {
        let session = bws::login_client_with_access_token(access_token).await?;
        let organization_id = session
            .client()
            .get_access_token_organization()
            .ok_or_else(|| anyhow::anyhow!("bitwarden organization is missing in access token"))?
            .into();
        let projects = session
            .client()
            .projects()
            .list(&ProjectsListRequest { organization_id })
            .await
            .map_err(|_| anyhow::anyhow!("bitwarden project list failed"))?;
        Ok(projects
            .data
            .into_iter()
            .map(|project| BwsLookupCandidate {
                id: BwsProjectId::new(project.id.to_string()),
                name: project.name,
            })
            .collect())
    }

    /// SDK secret 一覧を指定 project 内の port 境界 lookup 候補へ変換する。
    async fn list_bws_secrets(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
    ) -> crate::Result<Vec<BwsLookupCandidate<BwsSecretId>>> {
        let session = bws::login_client_with_access_token(access_token).await?;
        let project_id = parse_uuid(project_id.as_str(), "bws project id")?;
        let secrets = session
            .client()
            .secrets()
            .list_by_project(&SecretIdentifiersByProjectRequest { project_id })
            .await
            .map_err(|_| anyhow::anyhow!("bitwarden secret list failed"))?;
        Ok(secrets
            .data
            .into_iter()
            .map(|secret| BwsLookupCandidate {
                id: BwsSecretId::new(secret.id.to_string()),
                name: secret.key,
            })
            .collect())
    }

    /// port 境界の secret ID を SDK UUID へ変換し、保護済み secret として application へ戻す。
    async fn fetch_bws_secret_by_id(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<ProtectedSecret> {
        let session = bws::login_client_with_access_token(access_token).await?;
        let id = parse_uuid(secret_id.as_str(), "bws secret id")?;
        session.get_protected_secret_value(id).await
    }
}

/// port 境界の opaque ID を Bitwarden SDK が要求する UUID 型へ翻訳する。
///
/// ID の一意性や対象同一性は domain で判定済みとし、ここでは SDK 型変換の失敗だけを扱う。
#[cfg(not(feature = "secrets-internal-test-stub"))]
fn parse_uuid(value: &str, label: &str) -> crate::Result<Uuid> {
    value
        .parse()
        .map_err(|_| anyhow::anyhow!("{label} is not a valid UUID"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::domain::values::BwsSecretName;

    /// BWS secret の domain 名を Bitwarden Secrets Manager の固定 key へ翻訳する。
    #[test]
    fn bws_secret_name_maps_to_stable_key() {
        assert_eq!(
            BwsSecretName::GpgSecretKeyBackup.key(),
            "gpg-secret-key-backup"
        );
        assert_eq!(
            BwsSecretName::PasswordStoreRemote.key(),
            "password-store-remote"
        );
    }

    /// adapter の default 構築が runtime 状態や外部接続を開始しないことを確認する。
    #[test]
    fn adapter_constructs_with_default() {
        let _ = BwsClientAdapter;
    }
}
