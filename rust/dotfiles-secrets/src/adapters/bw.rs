//! `BwsClientPort` を Bitwarden Secrets Manager 取得境界へ接続する adapter。
//!
//! application は BWS lookup plan と domain の一意解決規則を保持する。adapter は SDK API の
//! project/secret/list/get 境界を port の ID 候補と保護済み secret へ翻訳する。
//!
//! # 一次資料と採用フロー
//!
//! この adapter の SDK 利用フロー・権限境界・エラー方針は
//! [`docs/secret-recovery/external-sdk-evidence.md`](../../../../docs/secret-recovery/external-sdk-evidence.md)
//! の「Bitwarden Secrets Manager SDK」を正本とする。同文書は Bitwarden の Secrets
//! Manager 全体資料、machine-account / project の権限資料、SDK 2.1.0 とその公開する
//! `bitwarden-sm` 3.0.0 の version 固定 source を直接参照している。
//!
//! `Client::new` → `auth().login_access_token` → token に結び付く organization を取得 →
//! `projects().list` / `secrets().list_by_project` → `secrets().get` / `create` / `update`
//! の順を守る。SDK の `SecretsManagerError` は validation / crypto / chrono / API /
//! missing-field を区別するため、ここでは retry・不存在・権限失敗などへ再分類しない。
//! context を付ける場合も source error を保持して伝播する。

#[cfg(not(feature = "secrets-internal-test-stub"))]
use anyhow::Context;
#[cfg(not(feature = "secrets-internal-test-stub"))]
use bitwarden::secrets_manager::{
    projects::ProjectsListRequest,
    secrets::{SecretGetRequest, SecretIdentifiersByProjectRequest, SecretPutRequest},
};

#[cfg(not(feature = "secrets-internal-test-stub"))]
use crate::{
    domain::{
        bws::{BwsLookupCandidate, BwsProjectId, BwsSecretId, BwsSecretName},
        gpg_backup::{BackupUpdateGuard, GpgBackupEnvelope},
        pass_restore::PasswordStoreRemote,
    },
    ports::bw::BwsClientPort,
    support::{
        adapter_backend::BwsClientBackend,
        bws_sdk,
        protection::{ProtectedSecret, bws},
    },
};

#[cfg(not(feature = "secrets-internal-test-stub"))]
impl BwsClientPort for BwsClientBackend {
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
                organization_id: bws_sdk::access_token_scope_id(&session)?,
            })
            .await
            .context("Bitwarden SDK project list failed")?;
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
        let project_id = bws_sdk::parse_uuid(project_id.as_str(), "bws project id")?;
        let secrets = session
            .client()
            .secrets()
            .list_by_project(&SecretIdentifiersByProjectRequest { project_id })
            .await
            .context("Bitwarden SDK project secret list failed")?;
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
        let id = bws_sdk::parse_uuid(secret_id.as_str(), "bws secret id")?;
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
        let id = bws_sdk::parse_uuid(secret_id.as_str(), "bws secret id")?;
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
        let sdk_scope_id = bws_sdk::access_token_scope_id(&session)?;
        let project_uuid = bws_sdk::parse_uuid(project_id.as_str(), "bws project id")?;
        let value = envelope.to_json_string()?;
        let created = session
            .client()
            .secrets()
            .create(&bws_sdk::secret_create_request(
                sdk_scope_id,
                project_uuid,
                BwsSecretName::GpgSecretKeyBackup.key(),
                value,
            ))
            .await
            .context("Bitwarden SDK secret create failed")?;
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
        let project_uuid = bws_sdk::parse_uuid(project_id.as_str(), "bws project id")?;
        let id = bws_sdk::parse_uuid(secret_id.as_str(), "bws secret id")?;
        let current = session
            .client()
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .context("Bitwarden SDK secret get before guarded update failed")?;
        let current_guard = BackupUpdateGuard::from_revision_or_value(
            current.revision_date.to_rfc3339(),
            current.value.as_bytes(),
        );
        expected_guard.ensure_matches(&current_guard)?;
        let current_project_id = current
            .project_id
            .ok_or_else(|| anyhow::anyhow!("Bitwarden SDK secret response omitted project_id"))?;
        if current_project_id != project_uuid {
            anyhow::bail!("Bitwarden SDK secret project changed before guarded update");
        }
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
                // A BWS secret is associated with a project.  Do not invent a
                // missing response value by substituting the caller's project:
                // the source response is required to preserve its assignment.
                project_ids: Some(vec![current_project_id]),
            })
            .await
            .context("Bitwarden SDK guarded secret update failed")?;
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
        let sdk_scope_id = bws_sdk::access_token_scope_id(&session)?;
        let project_uuid = bws_sdk::parse_uuid(project_id.as_str(), "bws project id")?;
        let created = session
            .client()
            .secrets()
            .create(&bws_sdk::secret_create_request(
                sdk_scope_id,
                project_uuid,
                BwsSecretName::PasswordStoreRemote.key(),
                remote.as_str().to_owned(),
            ))
            .await
            .context("Bitwarden SDK secret create failed")?;
        Ok(BwsSecretId::new(created.id.to_string()))
    }

    /// 現行 `password-store-remote` の SDK revision（取得不可なら value digest）を guard 化して返す。
    async fn fetch_password_store_remote_guard(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<BackupUpdateGuard> {
        let session = bws::login_client_with_access_token(access_token).await?;
        let id = bws_sdk::parse_uuid(secret_id.as_str(), "bws secret id")?;
        let secret = session
            .client()
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .context("Bitwarden SDK secret get for update guard failed")?;
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
        let project_uuid = bws_sdk::parse_uuid(project_id.as_str(), "bws project id")?;
        let id = bws_sdk::parse_uuid(secret_id.as_str(), "bws secret id")?;
        let current = session
            .client()
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .context("Bitwarden SDK secret get before guarded update failed")?;
        let current_guard = BackupUpdateGuard::from_revision_or_value(
            current.revision_date.to_rfc3339(),
            current.value.as_bytes(),
        );
        expected_guard.ensure_matches(&current_guard)?;
        let current_project_id = current
            .project_id
            .ok_or_else(|| anyhow::anyhow!("Bitwarden SDK secret response omitted project_id"))?;
        if current_project_id != project_uuid {
            anyhow::bail!("Bitwarden SDK secret project changed before guarded update");
        }
        session
            .client()
            .secrets()
            .update(&SecretPutRequest {
                id,
                organization_id: current.organization_id,
                key: BwsSecretName::PasswordStoreRemote.key().to_owned(),
                value: remote.as_str().to_owned(),
                note: current.note.clone(),
                // Preserve the project returned by the SDK.  Missing data is a
                // failure; using `project_uuid` here would be an unsupported
                // fallback that changes the remote secret's assignment.
                project_ids: Some(vec![current_project_id]),
            })
            .await
            .context("Bitwarden SDK guarded secret update failed")?;
        Ok(())
    }
}
