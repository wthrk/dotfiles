//! Bitwarden Secrets Manager concrete SDK backend operations.
//!
//! ## 出典と適用判断
//!
//! repository 正本は [`secret-recovery-spec.md`](../../../docs/secret-recovery/secret-recovery-spec.md)
//! の「無対話復旧の利用者契約」「Secret の置き場所」「Bitwarden Secrets Manager」と
//! [`bitwarden-personal-vault-design.md`](../../../docs/secret-recovery/bitwarden-personal-vault-design.md)
//! である。この backend は application/domain が解決した project/secret ID と key を
//! SDK request に渡す技術操作だけを担当する。project/secret 名の一意解決、必須性、
//! recovery flow、0 件/複数件の業務上の扱いをここで決めない。
//!
//! vendor 全体フローは [Bitwarden Developer Quick Start](https://bitwarden.com/help/developer-quick-start/)
//! と [Secrets Manager SDK](https://bitwarden.com/help/secrets-manager-sdk/) を、machine-account
//! の project 権限は [Machine Accounts](https://bitwarden.com/help/machine-accounts/) を直接確認する。
//! 現コードが使う version 固定 SDK API は `bitwarden-sm` 3.0.0
//! [`ProjectsClient::list`](https://docs.rs/crate/bitwarden-sm/3.0.0/source/src/client_projects.rs)、
//! [`SecretsClient::list_by_project` / `get` / `create` / `update`](https://docs.rs/crate/bitwarden-sm/3.0.0/source/src/client_secrets.rs)、
//! [`SecretsManagerError`](https://docs.rs/crate/bitwarden-sm/3.0.0/source/src/error.rs) である。
//! 各 call の `Result` は operation context を加える場合も source error を保持して伝播し、
//! error text や未確認 status を not-found、permission、transient、success、空結果に
//! 再分類しない。Password Manager `bw` CLI の login/session/OTP flow は別製品面であり、
//! この BWS backend の fallback・入力・出力に採用しない。

use anyhow::Context;
use bitwarden::secrets_manager::{
    projects::ProjectsListRequest, secrets::SecretIdentifiersByProjectRequest,
};

use crate::{
    domain::{
        bws::{BwsLookupCandidate, BwsProjectId, BwsSecretId},
        gpg_backup::{BackupUpdateGuard, GpgBackupEnvelope},
        pass_restore::PasswordStoreRemote,
    },
    support::{
        bws_sdk,
        protection::{ProtectedSecret, bws},
    },
};

pub(crate) async fn list_bws_projects(
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
pub(crate) async fn list_bws_secrets(
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
pub(crate) async fn fetch_gpg_backup_envelope(
    access_token: &ProtectedSecret,
    secret_id: &BwsSecretId,
) -> crate::Result<(GpgBackupEnvelope, BackupUpdateGuard)> {
    let session = bws::login_client_with_access_token(access_token).await?;
    let id = bws_sdk::parse_uuid(secret_id.as_str(), "bws secret id")?;
    let (value, guard) = session.read_secret_value_with_revision(id).await?;
    let envelope = bws::parse_response_value(&value, |value| {
        GpgBackupEnvelope::from_json(value.as_bytes())
    })?;
    Ok((envelope, guard))
}
pub(crate) async fn fetch_password_store_remote(
    access_token: &ProtectedSecret,
    secret_id: &BwsSecretId,
) -> crate::Result<PasswordStoreRemote> {
    let session = bws::login_client_with_access_token(access_token).await?;
    let id = bws_sdk::parse_uuid(secret_id.as_str(), "bws secret id")?;
    let value = session.read_secret_value(id).await?;
    bws::parse_response_value(&value, PasswordStoreRemote::parse)
}
pub(crate) async fn create_gpg_backup_envelope(
    access_token: &ProtectedSecret,
    project_id: &BwsProjectId,
    secret_key: &str,
    envelope: &GpgBackupEnvelope,
) -> crate::Result<BwsSecretId> {
    let session = bws::login_client_with_access_token(access_token).await?;
    let scope = bws_sdk::access_token_scope_id(&session)?;
    let project = bws_sdk::parse_uuid(project_id.as_str(), "bws project id")?;
    let created = session
        .create_gpg_backup_envelope(scope, project, secret_key, envelope)
        .await?;
    Ok(BwsSecretId::new(created.to_string()))
}
pub(crate) async fn update_gpg_backup_envelope_if_unchanged(
    access_token: &ProtectedSecret,
    project_id: &BwsProjectId,
    secret_id: &BwsSecretId,
    secret_key: &str,
    envelope: &GpgBackupEnvelope,
    expected_guard: &BackupUpdateGuard,
) -> crate::Result<()> {
    let session = bws::login_client_with_access_token(access_token).await?;
    session
        .update_gpg_backup_envelope_if_unchanged(
            bws_sdk::parse_uuid(secret_id.as_str(), "bws secret id")?,
            bws_sdk::parse_uuid(project_id.as_str(), "bws project id")?,
            secret_key.to_owned(),
            envelope,
            expected_guard,
        )
        .await
}
pub(crate) async fn create_password_store_remote(
    access_token: &ProtectedSecret,
    project_id: &BwsProjectId,
    secret_key: &str,
    remote: &PasswordStoreRemote,
) -> crate::Result<BwsSecretId> {
    let session = bws::login_client_with_access_token(access_token).await?;
    let created = session
        .create_password_store_remote(
            bws_sdk::access_token_scope_id(&session)?,
            bws_sdk::parse_uuid(project_id.as_str(), "bws project id")?,
            secret_key,
            remote,
        )
        .await?;
    Ok(BwsSecretId::new(created.to_string()))
}
pub(crate) async fn fetch_password_store_remote_guard(
    access_token: &ProtectedSecret,
    secret_id: &BwsSecretId,
) -> crate::Result<BackupUpdateGuard> {
    let session = bws::login_client_with_access_token(access_token).await?;
    session
        .current_secret_guard(bws_sdk::parse_uuid(secret_id.as_str(), "bws secret id")?)
        .await
}
pub(crate) async fn update_password_store_remote_if_unchanged(
    access_token: &ProtectedSecret,
    project_id: &BwsProjectId,
    secret_id: &BwsSecretId,
    secret_key: &str,
    remote: &PasswordStoreRemote,
    expected_guard: &BackupUpdateGuard,
) -> crate::Result<()> {
    let session = bws::login_client_with_access_token(access_token).await?;
    session
        .update_password_store_remote_if_unchanged(
            bws_sdk::parse_uuid(secret_id.as_str(), "bws secret id")?,
            bws_sdk::parse_uuid(project_id.as_str(), "bws project id")?,
            secret_key.to_owned(),
            remote,
            expected_guard,
        )
        .await
}
