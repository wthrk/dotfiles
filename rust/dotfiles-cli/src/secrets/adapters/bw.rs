//! `BwsClientPort` を Bitwarden Secrets Manager 取得境界へ接続する adapter。
//!
//! application は BWS lookup plan と domain の一意解決規則を保持する。adapter は SDK API の
//! project/secret/list/get 境界を port の ID 候補と保護済み secret へ翻訳する。

#[cfg(feature = "secrets-internal-test-stub")]
mod internal_stub;
// `secrets-internal-test-stub` feature 専用の BWS adapter backend stub。
//
// production build には含めず、runtime real/stub 分岐は作らない。integration test は adapter
// stub module を import せず、feature 有効でビルドされた同じ `dotfiles` binary を実行し、
// BWS port 専用の初期条件 spec JSON と最終状態観測 JSON だけを外部観測面として扱う。

#[cfg(not(feature = "secrets-internal-test-stub"))]
use bitwarden::secrets_manager::{
    projects::ProjectsListRequest,
    secrets::{
        SecretCreateRequest, SecretGetRequest, SecretIdentifiersByProjectRequest, SecretPutRequest,
    },
};
#[cfg(not(feature = "secrets-internal-test-stub"))]
use uuid::Uuid;

#[cfg(not(feature = "secrets-internal-test-stub"))]
use crate::secrets::{
    domain::{
        bws::{BwsLookupCandidate, BwsProjectId, BwsSecretId},
        gpg_backup::{BackupUpdateGuard, GpgBackupEnvelope},
    },
    ports::bw::BwsClientPort,
    support::protection::{ProtectedSecret, bws},
};

/// Bitwarden Secrets Manager SDK を `BwsClientPort` へ翻訳する adapter。
#[derive(Default)]
pub(in crate::secrets) struct BwsClientAdapter;

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

    /// secret value（encrypted envelope JSON）と SDK revision を取得し、domain envelope + guard へ翻訳する。
    async fn fetch_gpg_backup_envelope(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<(GpgBackupEnvelope, BackupUpdateGuard)> {
        let session = bws::login_client_with_access_token(access_token).await?;
        let id = parse_uuid(secret_id.as_str(), "bws secret id")?;
        let secret = session
            .client()
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .map_err(|_| anyhow::anyhow!("bitwarden secret get failed"))?;
        let envelope = GpgBackupEnvelope::from_json(secret.value.as_bytes())?;
        // SDK revision（updatedAt 相当）を更新識別子として guard 化する。取得できない場合は value digest。
        let guard = BackupUpdateGuard::from_revision(secret.revision_date.to_rfc3339())
            .unwrap_or_else(|| BackupUpdateGuard::from_value_bytes(secret.value.as_bytes()));
        Ok((envelope, guard))
    }

    /// 指定 project に新しい envelope secret を作成し、その ID を port 境界の opaque 値として返す。
    async fn create_gpg_backup_envelope(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
        key: &str,
        envelope: &GpgBackupEnvelope,
    ) -> crate::Result<BwsSecretId> {
        let session = bws::login_client_with_access_token(access_token).await?;
        let organization_id = session
            .client()
            .get_access_token_organization()
            .ok_or_else(|| anyhow::anyhow!("bitwarden organization is missing in access token"))?
            .into();
        let project_uuid = parse_uuid(project_id.as_str(), "bws project id")?;
        let value = envelope_value(envelope)?;
        let created = session
            .client()
            .secrets()
            .create(&SecretCreateRequest {
                organization_id,
                key: key.to_owned(),
                value,
                note: String::new(),
                project_ids: Some(vec![project_uuid]),
            })
            .await
            .map_err(|_| anyhow::anyhow!("bitwarden secret create failed"))?;
        Ok(BwsSecretId::new(created.id.to_string()))
    }

    /// 更新直前に現行 revision を再取得し、guard 一致を確認した場合だけ envelope を上書き更新する。
    async fn update_gpg_backup_envelope_if_unchanged(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
        secret_id: &BwsSecretId,
        key: &str,
        envelope: &GpgBackupEnvelope,
        expected_guard: &BackupUpdateGuard,
    ) -> crate::Result<()> {
        let session = bws::login_client_with_access_token(access_token).await?;
        let organization_id = session
            .client()
            .get_access_token_organization()
            .ok_or_else(|| anyhow::anyhow!("bitwarden organization is missing in access token"))?
            .into();
        let project_uuid = parse_uuid(project_id.as_str(), "bws project id")?;
        let id = parse_uuid(secret_id.as_str(), "bws secret id")?;
        // 更新直前に現行値を再取得し、guard 一致を stale overwrite 防止条件として確認する。
        let current = session
            .client()
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .map_err(|_| anyhow::anyhow!("bitwarden secret get failed"))?;
        let current_guard = BackupUpdateGuard::from_revision(current.revision_date.to_rfc3339())
            .unwrap_or_else(|| BackupUpdateGuard::from_value_bytes(current.value.as_bytes()));
        expected_guard.ensure_matches(&current_guard)?;
        let value = envelope_value(envelope)?;
        session
            .client()
            .secrets()
            .update(&SecretPutRequest {
                id,
                organization_id,
                key: key.to_owned(),
                value,
                note: String::new(),
                project_ids: Some(vec![project_uuid]),
            })
            .await
            .map_err(|_| anyhow::anyhow!("bitwarden secret update failed"))?;
        Ok(())
    }
}

/// 検証済み envelope を canonical UTF-8 JSON 文字列へ serialize する。encrypted envelope であり平文鍵素材を含まない。
#[cfg(not(feature = "secrets-internal-test-stub"))]
fn envelope_value(envelope: &GpgBackupEnvelope) -> crate::Result<String> {
    String::from_utf8(envelope.to_json()?)
        .map_err(|_| anyhow::anyhow!("gpg backup envelope is not valid UTF-8"))
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
    use crate::secrets::domain::bws::BwsSecretName;

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
