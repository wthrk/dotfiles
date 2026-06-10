//! `BwsClientPort` を Bitwarden Secrets Manager 取得境界へ接続する adapter。
//!
//! application は BWS lookup plan と domain の一意解決規則を保持する。adapter は SDK API の
//! project/secret/list/get 境界を port の ID 候補と保護済み secret へ翻訳する。
#[cfg(not(feature = "secrets-internal-test-stub"))]
use anyhow::Context;

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
    projects::{ProjectCreateRequest, ProjectsListRequest},
    secrets::{SecretCreateRequest, SecretIdentifiersByProjectRequest},
};
#[cfg(not(feature = "secrets-internal-test-stub"))]
use uuid::Uuid;

#[cfg(not(feature = "secrets-internal-test-stub"))]
use crate::secrets::{
    domain::{
        bws::{BwsLookupCandidate, BwsProjectId, BwsProjectName, BwsSecretId, BwsSecretName},
        gpg_backup::GpgBackupEnvelope,
        pass_restore::PasswordStoreRemote,
    },
    ports::bw::BwsClientPort,
    support::protection::{ProtectedSecret, bws},
};

/// Bitwarden Secrets Manager SDK を `BwsClientPort` へ翻訳する adapter。
#[derive(Default)]
pub(in crate::secrets) struct BwsClientAdapter;

/// `bw` CLI（Bitwarden Password Manager）の login / unlock を `BwLoginPort` へ翻訳する adapter。
///
/// real backend（`login_adapter`）は `bw login` / `bw unlock` の子プロセスを起動し、stub backend
/// （`login_stub`）は子プロセスを起動せず datastore 遷移として模す。impl は backend module 側に閉じる。
#[derive(Default)]
pub(in crate::secrets) struct BwLoginAdapter;

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
        .with_context(|| format!("{label} is not a valid UUID"))
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
fn secret_create_request(
    organization_id: Uuid,
    project_id: Uuid,
    key: &str,
    value: String,
    note: String,
) -> SecretCreateRequest {
    SecretCreateRequest {
        organization_id,
        key: key.to_owned(),
        value,
        note,
        project_ids: Some(vec![project_id]),
    }
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
async fn login_bws_session(
    access_token: &ProtectedSecret,
    operation: impl FnOnce() -> String,
) -> crate::Result<bws::BwsClientSession> {
    bws::login_client_with_access_token(access_token)
        .await
        .with_context(operation)
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
impl BwsClientAdapter {
    /// `password-store-remote` secret note の provenance marker を adapter 境界内で取り出す。
    async fn fetch_password_store_remote_note_marker(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<Option<String>> {
        let session = login_bws_session(access_token, || {
            format!(
                "BWS client adapter failed to fetch secret `{}` as `password-store-remote` note",
                secret_id.as_str()
            )
        })
        .await?;
        let id = parse_sdk_uuid(secret_id.as_str(), "bws secret id")?;
        let note = session.get_non_secret_note(id).await?;
        Ok(bws::parse_provisioning_token_note(note.as_str()).map(str::to_owned))
    }
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
impl BwsClientPort for BwsClientAdapter {
    /// SDK project 一覧を port 境界の lookup 候補へ変換する。
    async fn list_bws_projects(
        &self,
        access_token: &ProtectedSecret,
    ) -> crate::Result<Vec<BwsLookupCandidate<BwsProjectId>>> {
        let session = login_bws_session(access_token, || {
            "BWS client adapter failed to list projects".into()
        })
        .await?;
        let projects = session
            .client()
            .projects()
            .list(&ProjectsListRequest {
                organization_id: access_token_scope_id(&session)?,
            })
            .await
            .context("BWS client adapter failed to list projects")?;
        Ok(projects
            .data
            .into_iter()
            .map(|project| BwsLookupCandidate {
                id: BwsProjectId::new(project.id.to_string()),
                name: project.name,
            })
            .collect())
    }

    /// SDK project create を port 境界の opaque project ID へ変換する。
    async fn create_bws_project(
        &self,
        access_token: &ProtectedSecret,
        project_name: BwsProjectName,
    ) -> crate::Result<BwsProjectId> {
        let session = login_bws_session(access_token, || {
            format!(
                "BWS client adapter failed to create project `{}`",
                project_name.as_str()
            )
        })
        .await?;
        let created = session
            .client()
            .projects()
            .create(&ProjectCreateRequest {
                organization_id: access_token_scope_id(&session)?,
                name: project_name.as_str().to_owned(),
            })
            .await
            .with_context(|| {
                format!(
                    "BWS client adapter failed to create project `{}`",
                    project_name.as_str()
                )
            })?;
        Ok(BwsProjectId::new(created.id.to_string()))
    }

    /// SDK secret 一覧を指定 project 内の port 境界 lookup 候補へ変換する。
    async fn list_bws_secrets(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
    ) -> crate::Result<Vec<BwsLookupCandidate<BwsSecretId>>> {
        let session = login_bws_session(access_token, || {
            format!(
                "BWS client adapter failed to list secrets in project `{}`",
                project_id.as_str()
            )
        })
        .await?;
        let project_id = parse_sdk_uuid(project_id.as_str(), "bws project id")?;
        let secrets = session
            .client()
            .secrets()
            .list_by_project(&SecretIdentifiersByProjectRequest { project_id })
            .await
            .with_context(|| {
                format!(
                    "BWS client adapter failed to list secrets in project `{}`",
                    project_id
                )
            })?;
        Ok(secrets
            .data
            .into_iter()
            .map(|secret| BwsLookupCandidate {
                id: BwsSecretId::new(secret.id.to_string()),
                name: secret.key,
            })
            .collect())
    }

    /// secret value（encrypted envelope JSON）を取得し、domain envelope へ翻訳する。
    async fn fetch_gpg_backup_envelope(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<GpgBackupEnvelope> {
        let session = login_bws_session(access_token, || {
            format!(
                "BWS client adapter failed to fetch secret `{}` as `gpg-secret-key-backup`",
                secret_id.as_str()
            )
        })
        .await?;
        let id = parse_sdk_uuid(secret_id.as_str(), "bws secret id")?;
        session
            .parse_secret_value_with_revision(id, |json, _revision| {
                GpgBackupEnvelope::from_json(json.as_bytes())
            })
            .await
            .with_context(|| {
                format!(
                    "BWS client adapter failed to fetch secret `{}` as `gpg-secret-key-backup`",
                    secret_id.as_str()
                )
            })
    }

    /// `password-store-remote` secret value を取得し、adapter 翻訳として domain 検証した clone URL を返す。
    async fn fetch_password_store_remote(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<PasswordStoreRemote> {
        let session = login_bws_session(access_token, || {
            format!(
                "BWS client adapter failed to fetch secret `{}` as `password-store-remote`",
                secret_id.as_str()
            )
        })
        .await?;
        let id = parse_sdk_uuid(secret_id.as_str(), "bws secret id")?;
        let value = session.get_non_secret_value(id).await.with_context(|| {
            format!(
                "BWS client adapter failed to fetch secret `{}` as `password-store-remote`",
                secret_id.as_str()
            )
        })?;
        PasswordStoreRemote::parse(value.as_str())
    }

    /// 候補 `bws-access-token` の provenance gate を、token id 抽出と BWS note 取得を含めて adapter 境界内で完了する。
    async fn ensure_recovery_token_provenance(
        &self,
        access_token: &ProtectedSecret,
    ) -> crate::Result<()> {
        let project_id = BwsProjectName::DOTFILES_SECRET_RECOVERY
            .resolve_id(self.list_bws_projects(access_token).await?)?;
        let secret_id = BwsSecretName::PasswordStoreRemote.resolve_id(
            self.list_bws_secrets(access_token, &project_id).await?,
            &project_id,
        )?;
        let note_marker = self
            .fetch_password_store_remote_note_marker(access_token, &secret_id)
            .await
            .context(
                "BWS client adapter failed to fetch `password-store-remote` provenance marker",
            )?;
        bws::ensure_recovery_token_allowed(access_token, note_marker.as_deref())
    }

    /// 検証済み clone URL を指定 project に新しい secret として作成する。
    async fn create_password_store_remote(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
        remote: &PasswordStoreRemote,
    ) -> crate::Result<BwsSecretId> {
        let session = login_bws_session(access_token, || {
            "BWS client adapter failed to create secret `password-store-remote`".into()
        })
        .await?;
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
                bws::provisioning_token_note(access_token)?,
            ))
            .await
            .context("BWS client adapter failed to create secret `password-store-remote`")?;
        Ok(BwsSecretId::new(created.id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::{
        domain::bws::{BwsProjectId, BwsProjectName},
        ports::bw::BwsClientPort,
        support::protection::ProtectedSecret,
    };

    /// adapter の default 構築が runtime 状態や外部接続を開始しないことを確認する。
    #[test]
    fn adapter_constructs_with_default() {
        let _ = BwsClientAdapter;
    }

    fn protected_secret(bytes: &[u8]) -> ProtectedSecret {
        match ProtectedSecret::from_test_bytes(bytes) {
            Ok(secret) => secret,
            Err(error) => panic!("failed to create test secret: {error}"),
        }
    }

    #[tokio::test]
    async fn list_bws_projects_wraps_login_failure_with_adapter_context() {
        let adapter = BwsClientAdapter;
        let error = match adapter.list_bws_projects(&protected_secret(b"\n")).await {
            Ok(_) => panic!("expected list_bws_projects to fail"),
            Err(error) => error,
        };
        let rendered = format!("{error:#}");

        assert!(
            rendered.contains("BWS client adapter failed to list projects"),
            "{rendered}"
        );
        assert!(
            rendered.contains("bws access token must not be empty"),
            "{rendered}"
        );
    }

    #[tokio::test]
    async fn create_bws_project_wraps_login_failure_with_adapter_context() {
        let adapter = BwsClientAdapter;
        let error = match adapter
            .create_bws_project(
                &protected_secret(b"\n"),
                BwsProjectName::DOTFILES_SECRET_RECOVERY,
            )
            .await
        {
            Ok(_) => panic!("expected create_bws_project to fail"),
            Err(error) => error,
        };
        let rendered = format!("{error:#}");

        assert!(
            rendered
                .contains("BWS client adapter failed to create project `dotfiles-secret-recovery`"),
            "{rendered}"
        );
        assert!(
            rendered.contains("bws access token must not be empty"),
            "{rendered}"
        );
    }

    #[tokio::test]
    async fn list_bws_secrets_wraps_login_failure_with_adapter_context() {
        let adapter = BwsClientAdapter;
        let error = match adapter
            .list_bws_secrets(
                &protected_secret(b"\n"),
                &BwsProjectId::new("11111111-1111-1111-1111-111111111111"),
            )
            .await
        {
            Ok(_) => panic!("expected list_bws_secrets to fail"),
            Err(error) => error,
        };
        let rendered = format!("{error:#}");

        assert!(
            rendered.contains(
                "BWS client adapter failed to list secrets in project `11111111-1111-1111-1111-111111111111`"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("bws access token must not be empty"),
            "{rendered}"
        );
    }
}
