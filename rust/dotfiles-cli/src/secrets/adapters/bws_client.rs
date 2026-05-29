//! `BwsClientPort` を Bitwarden Secrets Manager 取得境界へ接続する adapter。
//!
//! application は「BWS secret を取得する capability」だけを要求する。secret を扱う SDK 呼び出しは
//! support/protection 側の BWS 専用操作で完了させる。

use bitwarden::{
    Client,
    secrets_manager::{
        projects::ProjectsListRequest,
        secrets::{SecretGetRequest, SecretIdentifiersByProjectRequest},
    },
};
use uuid::Uuid;

use crate::{
    Result,
    secrets::{
        domain::{material::SecretMaterial, values::BwsSecretName},
        ports::{BwsClientPort, PortFuture},
        support::protection::{ProtectedSecret, bws},
    },
};

const BWS_PROJECT_NAME: &str = "dotfiles-secret-recovery";

#[derive(Default)]
pub(crate) struct BwsClientAdapter;

impl BwsClientPort for BwsClientAdapter {
    /// access token の生値を application 層へ返さず、protection 側の BWS 操作へ委譲する。
    fn fetch_bws_secret<'a>(
        &'a self,
        access_token: &'a SecretMaterial,
        secret_name: BwsSecretName,
    ) -> PortFuture<'a, SecretMaterial> {
        let protected = match access_token
            .as_backend::<ProtectedSecret>()
            .ok_or_else(|| anyhow::anyhow!("bws access token backend is not protected memory"))
        {
            Ok(protected) => protected,
            Err(error) => return Box::pin(async move { Err(error) }),
        };
        Box::pin(async move {
            let protected_value =
                fetch_secret_with_sdk(protected, BWS_PROJECT_NAME, bws_secret_key(secret_name))
                    .await?;
            Ok(SecretMaterial::from_backend(
                protected_value,
                ProtectedSecret::len,
                ProtectedSecret::try_clone,
            ))
        })
    }
}

/// BWS 認証、一意な project/secret 解決、取得値の保護値化を adapter 境界で完了する。
///
/// caller は `access_token` が `ProtectedSecret` backend であることを検証してから渡す。
/// SDK error と曖昧な project/secret 解決は固定要約の error へ変換し、SDK から返った
/// secret value はこの関数内で直ちに protection 側へ渡す。
async fn fetch_secret_with_sdk(
    access_token: &ProtectedSecret,
    project_name: &'static str,
    secret_key: &'static str,
) -> Result<ProtectedSecret> {
    let client = Client::new(None);
    bws::with_access_token_login_request(access_token, |request| {
        Box::pin(async move {
            client
                .auth()
                .login_access_token(request)
                .await
                .map_err(|_| anyhow::anyhow!("bitwarden login failed"))?;
            let organization_id = client
                .get_access_token_organization()
                .ok_or_else(|| {
                    anyhow::anyhow!("bitwarden organization is missing in access token")
                })?
                .into();
            let project_id = resolve_project_id(&client, organization_id, project_name).await?;
            let secret_id = resolve_secret_id(&client, &project_id, secret_key).await?;
            let secret = client
                .secrets()
                .get(&SecretGetRequest { id: secret_id })
                .await
                .map_err(|_| anyhow::anyhow!("bitwarden secret get failed"))?;
            bws::protect_secret_value(secret.value)
        })
    })
    .await
}

/// 認証済み BWS client から固定 project 名に一致する project ID を一意に解決する。
///
/// caller は login 済み client と access token organization ID を渡す。project が存在しない場合、
/// または同名 project が複数ある場合は SDK ID を返さず error にする。
async fn resolve_project_id(
    client: &Client,
    organization_id: Uuid,
    project_name: &'static str,
) -> Result<Uuid> {
    let projects = client
        .projects()
        .list(&ProjectsListRequest { organization_id })
        .await
        .map_err(|_| anyhow::anyhow!("bitwarden project list failed"))?;
    let mut matches = projects
        .data
        .into_iter()
        .filter(|project| project.name == project_name);
    let Some(project) = matches.next() else {
        return Err(anyhow::anyhow!("bws project not found: {project_name}"));
    };
    if matches.next().is_some() {
        return Err(anyhow::anyhow!(
            "multiple bws projects matched: {project_name}"
        ));
    }
    Ok(project.id)
}

/// 解決済み project 内で固定 secret key に一致する secret ID を一意に解決する。
///
/// caller は project ID の一意解決を先に完了してから呼ぶ。secret が存在しない場合、
/// または同名 key が複数ある場合は取得呼び出しへ進まず error にする。
async fn resolve_secret_id(
    client: &Client,
    project_id: &Uuid,
    secret_key: &'static str,
) -> Result<Uuid> {
    let secrets = client
        .secrets()
        .list_by_project(&SecretIdentifiersByProjectRequest {
            project_id: *project_id,
        })
        .await
        .map_err(|_| anyhow::anyhow!("bitwarden secret list failed"))?;
    let mut matches = secrets
        .data
        .into_iter()
        .filter(|secret| secret.key == secret_key);
    let Some(secret) = matches.next() else {
        return Err(anyhow::anyhow!(
            "bws secret key not found in project {project_id}: {secret_key}"
        ));
    };
    if matches.next().is_some() {
        return Err(anyhow::anyhow!(
            "multiple bws secret keys matched in project {project_id}: {secret_key}"
        ));
    }
    Ok(secret.id)
}

fn bws_secret_key(secret_name: BwsSecretName) -> &'static str {
    match secret_name {
        BwsSecretName::GpgSecretKeyBackup => "gpg-secret-key-backup",
        BwsSecretName::PasswordStoreRemote => "password-store-remote",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BWS secret の domain 名を Bitwarden Secrets Manager の固定 key へ翻訳する。
    #[test]
    fn bws_secret_name_maps_to_stable_key() {
        assert_eq!(
            bws_secret_key(BwsSecretName::GpgSecretKeyBackup),
            "gpg-secret-key-backup"
        );
        assert_eq!(
            bws_secret_key(BwsSecretName::PasswordStoreRemote),
            "password-store-remote"
        );
    }

    /// adapter の default 構築が runtime 状態や外部接続を開始しないことを確認する。
    #[test]
    fn adapter_constructs_with_default() {
        let _ = BwsClientAdapter;
    }

    #[tokio::test]
    async fn rejects_unprotected_access_token_backend_before_sdk_call() {
        let token = SecretMaterial::from_backend((), |_| 0, |_| Ok(()));
        let adapter = BwsClientAdapter;
        let result = adapter
            .fetch_bws_secret(&token, BwsSecretName::GpgSecretKeyBackup)
            .await;

        match result {
            Ok(_) => panic!("unprotected bws access token unexpectedly accepted"),
            Err(error) => assert_eq!(
                error.to_string(),
                "bws access token backend is not protected memory"
            ),
        }
    }
}
