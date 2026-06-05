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

// `bw` CLI（Bitwarden Password Manager）の login / unlock 用 adapter backend。real backend は `bw login` /
// `bw unlock` の子プロセスを起動し、stub backend は子プロセスを起動せず datastore 遷移として模す。BWS SDK
// 経路（`BwsClientAdapter`）とは port / backend / 観測面を共有しない。
#[cfg(not(feature = "secrets-internal-test-stub"))]
mod login_adapter;
#[cfg(feature = "secrets-internal-test-stub")]
mod login_stub;

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
use crate::{
    domain::{
        bws::{BwsLookupCandidate, BwsProjectId, BwsSecretId, BwsSecretName},
        gpg_backup::{BackupUpdateGuard, GpgBackupEnvelope},
        pass_restore::PasswordStoreRemote,
    },
    ports::bw::BwsClientPort,
    support::protection::{ProtectedSecret, bws},
};

/// Bitwarden Secrets Manager SDK を `BwsClientPort` へ翻訳する adapter。
#[derive(Default)]
pub(crate) struct BwsClientAdapter;

/// `bw` CLI（Bitwarden Password Manager）の login / unlock を `BwLoginPort` へ翻訳する adapter。
///
/// real backend（`login_adapter`）は `bw login` / `bw unlock` の子プロセスを起動し、stub backend
/// （`login_stub`）は子プロセスを起動せず datastore 遷移として模す。impl は backend module 側に閉じる。
#[derive(Default)]
pub(crate) struct BwLoginAdapter;

#[cfg(not(feature = "secrets-internal-test-stub"))]
fn access_token_scope_id(session: &bws::BwsClientSession) -> crate::Result<Uuid> {
    session
        .client()
        .get_access_token_organization()
        .map(Into::into)
        .ok_or_else(|| anyhow::anyhow!("bitwarden access token does not expose a BWS SDK scope id"))
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
fn parse_sdk_uuid(value: &str, label: &str) -> crate::Result<Uuid> {
    value
        .parse()
        .map_err(|_| anyhow::anyhow!("{label} is not a valid UUID"))
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
fn secret_create_request(
    organization_id: Uuid,
    project_id: Uuid,
    key: &str,
    value: String,
) -> SecretCreateRequest {
    SecretCreateRequest {
        organization_id,
        key: key.to_owned(),
        value,
        note: String::new(),
        project_ids: Some(vec![project_id]),
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
        let projects = session
            .client()
            .projects()
            .list(&ProjectsListRequest {
                organization_id: access_token_scope_id(&session)?,
            })
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
        let project_id = parse_sdk_uuid(project_id.as_str(), "bws project id")?;
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

    /// secret value（encrypted envelope JSON）と SDK revision を取得し、domain envelope + guard へ翻訳する。
    async fn fetch_gpg_backup_envelope(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<(GpgBackupEnvelope, BackupUpdateGuard)> {
        let session = bws::login_client_with_access_token(access_token).await?;
        let id = parse_sdk_uuid(secret_id.as_str(), "bws secret id")?;
        let (envelope, guard) = session
            .parse_secret_value_with_revision(id, |json, revision| {
                let guard =
                    BackupUpdateGuard::from_revision_or_value(revision.to_owned(), json.as_bytes());
                let envelope = GpgBackupEnvelope::from_json(json.as_bytes())?;
                Ok((envelope, guard))
            })
            .await?;
        Ok((envelope, guard))
    }

    /// `password-store-remote` secret value を取得し、adapter 翻訳として domain 検証した clone URL を返す。
    async fn fetch_password_store_remote(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<PasswordStoreRemote> {
        let session = bws::login_client_with_access_token(access_token).await?;
        let id = parse_sdk_uuid(secret_id.as_str(), "bws secret id")?;
        let value = session.get_non_secret_value(id).await?;
        PasswordStoreRemote::parse(value.as_str())
    }

    /// 指定 project に新しい envelope secret を作成し、その ID を port 境界の opaque 値として返す。
    async fn create_gpg_backup_envelope(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
        envelope: &GpgBackupEnvelope,
    ) -> crate::Result<BwsSecretId> {
        let session = bws::login_client_with_access_token(access_token).await?;
        let sdk_scope_id = access_token_scope_id(&session)?;
        let project_uuid = parse_sdk_uuid(project_id.as_str(), "bws project id")?;
        let value = envelope.to_json_string()?;
        let created = session
            .client()
            .secrets()
            .create(&secret_create_request(
                sdk_scope_id,
                project_uuid,
                BwsSecretName::GpgSecretKeyBackup.key(),
                value,
            ))
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
        envelope: &GpgBackupEnvelope,
        expected_guard: &BackupUpdateGuard,
    ) -> crate::Result<()> {
        let session = bws::login_client_with_access_token(access_token).await?;
        let project_uuid = parse_sdk_uuid(project_id.as_str(), "bws project id")?;
        let id = parse_sdk_uuid(secret_id.as_str(), "bws secret id")?;
        let current = session
            .client()
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .map_err(|_| anyhow::anyhow!("bitwarden secret get failed"))?;
        let current_guard = BackupUpdateGuard::from_revision_or_value(
            current.revision_date.to_rfc3339(),
            current.value.as_bytes(),
        );
        expected_guard.ensure_matches(&current_guard)?;
        let value = envelope.to_json_string()?;
        session
            .client()
            .secrets()
            .update(&SecretPutRequest {
                id,
                organization_id: current.organization_id,
                key: BwsSecretName::GpgSecretKeyBackup.key().to_owned(),
                value,
                note: current.note.clone(),
                project_ids: Some(vec![current.project_id.unwrap_or(project_uuid)]),
            })
            .await
            .map_err(|_| anyhow::anyhow!("bitwarden secret update failed"))?;
        Ok(())
    }

    /// 検証済み clone URL を指定 project に新しい secret として作成する。
    async fn create_password_store_remote(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
        remote: &PasswordStoreRemote,
    ) -> crate::Result<BwsSecretId> {
        let session = bws::login_client_with_access_token(access_token).await?;
        let sdk_scope_id = access_token_scope_id(&session)?;
        let project_uuid = parse_sdk_uuid(project_id.as_str(), "bws project id")?;
        let created = session
            .client()
            .secrets()
            .create(&secret_create_request(
                sdk_scope_id,
                project_uuid,
                BwsSecretName::PasswordStoreRemote.key(),
                remote.as_str().to_owned(),
            ))
            .await
            .map_err(|_| anyhow::anyhow!("bitwarden secret create failed"))?;
        Ok(BwsSecretId::new(created.id.to_string()))
    }

    /// 現行 `password-store-remote` の SDK revision（取得不可なら value digest）を guard 化して返す。
    async fn fetch_password_store_remote_guard(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<BackupUpdateGuard> {
        let session = bws::login_client_with_access_token(access_token).await?;
        let id = parse_sdk_uuid(secret_id.as_str(), "bws secret id")?;
        let secret = session
            .client()
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .map_err(|_| anyhow::anyhow!("bitwarden secret get failed"))?;
        let guard = BackupUpdateGuard::from_revision_or_value(
            secret.revision_date.to_rfc3339(),
            secret.value.as_bytes(),
        );
        Ok(guard)
    }

    /// 更新直前に現行 revision を再取得し、guard 一致を確認した場合だけ clone URL を上書き更新する。
    async fn update_password_store_remote_if_unchanged(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
        secret_id: &BwsSecretId,
        remote: &PasswordStoreRemote,
        expected_guard: &BackupUpdateGuard,
    ) -> crate::Result<()> {
        let session = bws::login_client_with_access_token(access_token).await?;
        let project_uuid = parse_sdk_uuid(project_id.as_str(), "bws project id")?;
        let id = parse_sdk_uuid(secret_id.as_str(), "bws secret id")?;
        let current = session
            .client()
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .map_err(|_| anyhow::anyhow!("bitwarden secret get failed"))?;
        let current_guard = BackupUpdateGuard::from_revision_or_value(
            current.revision_date.to_rfc3339(),
            current.value.as_bytes(),
        );
        expected_guard.ensure_matches(&current_guard)?;
        session
            .client()
            .secrets()
            .update(&SecretPutRequest {
                id,
                organization_id: current.organization_id,
                key: BwsSecretName::PasswordStoreRemote.key().to_owned(),
                value: remote.as_str().to_owned(),
                note: current.note.clone(),
                project_ids: Some(vec![current.project_id.unwrap_or(project_uuid)]),
            })
            .await
            .map_err(|_| anyhow::anyhow!("bitwarden secret update failed"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// adapter の default 構築が runtime 状態や外部接続を開始しないことを確認する。
    #[test]
    fn adapter_constructs_with_default() {
        let _ = BwsClientAdapter;
    }
}
