//! compile-time BWS test backend の fixture datastore。

use crate::{
    Result,
    domain::{
        bws::{BwsLookupCandidate, BwsProjectId, BwsSecretId},
        gpg_backup::{BackupUpdateGuard, GpgBackupEnvelope},
        pass_restore::PasswordStoreRemote,
    },
    secrets_internal_test_stub_contract::{BWS_STUB_SPEC_ENV, STUB_OBSERVATION_PREFIX},
    support::protection::ProtectedSecret,
};
use anyhow::Context;
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};
#[derive(serde::Deserialize)]
struct Spec {
    fixture: Fixture,
    #[serde(default)]
    gpg_secret_key_backup: Option<String>,
    #[serde(default)]
    password_store_remote: Option<String>,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Fixture {
    DefaultRecoveryProject,
}
#[derive(Default)]
struct Store {
    projects: BTreeMap<String, String>,
    project_secrets: BTreeMap<String, BTreeMap<String, String>>,
    values: BTreeMap<String, String>,
}
#[derive(serde::Serialize)]
struct Observation {
    resolved_secrets: BTreeMap<String, String>,
}
#[derive(serde::Serialize)]
struct Frame<'a> {
    port: &'static str,
    observation: &'a Observation,
}
static STATE: OnceLock<Mutex<Option<Store>>> = OnceLock::new();
fn with_store<T>(f: impl FnOnce(&mut Store) -> Result<T>) -> Result<T> {
    let mut guard = STATE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| anyhow::anyhow!("BWS internal stub datastore lock is poisoned"))?;
    if guard.is_none() {
        *guard = Some(load()?)
    }
    let store = guard.as_mut().expect("initialized");
    let result = f(store)?;
    observe(store)?;
    Ok(result)
}
fn load() -> Result<Store> {
    let body = std::env::var(BWS_STUB_SPEC_ENV)
        .context("BWS internal stub spec JSON is not configured")?;
    let spec: Spec =
        serde_json::from_str(&body).context("failed to decode BWS internal stub spec JSON")?;
    let mut store = match spec.fixture {
        Fixture::DefaultRecoveryProject => default_store(),
    };
    if let Some(value) = spec.gpg_secret_key_backup {
        store.values.insert("bws-secret-id-gpg".into(), value);
    }
    if let Some(value) = spec.password_store_remote {
        store.values.insert("bws-secret-id-pass".into(), value);
    }
    Ok(store)
}
fn default_store() -> Store {
    let mut projects = BTreeMap::new();
    projects.insert(
        "bws-project-id-dotfiles".into(),
        "dotfiles-secret-recovery".into(),
    );
    let mut secrets = BTreeMap::new();
    secrets.insert("bws-secret-id-gpg".into(), "gpg-secret-key-backup".into());
    secrets.insert("bws-secret-id-pass".into(), "password-store-remote".into());
    let mut project_secrets = BTreeMap::new();
    project_secrets.insert("bws-project-id-dotfiles".into(), secrets);
    let mut values = BTreeMap::new();
    values.insert("bws-secret-id-access-token".into(), "token".into());
    values.insert("bws-secret-id-gpg".into(), "gpg-secret".into());
    values.insert(
        "bws-secret-id-pass".into(),
        "https://example.invalid/repo.git".into(),
    );
    Store {
        projects,
        project_secrets,
        values,
    }
}
fn check(token: &[u8], store: &Store) -> Result<()> {
    if store
        .values
        .get("bws-secret-id-access-token")
        .is_some_and(|value| value.as_bytes() == token)
    {
        Ok(())
    } else {
        anyhow::bail!("bitwarden login failed")
    }
}
fn observe(store: &Store) -> Result<()> {
    let mut resolved_secrets = BTreeMap::new();
    for secrets in store.project_secrets.values() {
        for (id, name) in secrets {
            if let Some(value) = store.values.get(id) {
                resolved_secrets.insert(name.clone(), value.clone());
            }
        }
    }
    println!(
        "{STUB_OBSERVATION_PREFIX}{}",
        serde_json::to_string(&Frame {
            port: "bws",
            observation: &Observation { resolved_secrets }
        })?
    );
    Ok(())
}
pub(crate) fn list_projects(token: &[u8]) -> Result<Vec<(String, String)>> {
    with_store(|store| {
        check(token, store)?;
        Ok(store
            .projects
            .iter()
            .map(|(id, name)| (id.clone(), name.clone()))
            .collect())
    })
}
pub(crate) fn list_project_secrets(token: &[u8], project: &str) -> Result<Vec<(String, String)>> {
    with_store(|store| {
        check(token, store)?;
        Ok(store
            .project_secrets
            .get(project)
            .ok_or_else(|| anyhow::anyhow!("bitwarden project not found"))?
            .iter()
            .map(|(id, name)| (id.clone(), name.clone()))
            .collect())
    })
}
pub(crate) fn read_secret(token: &[u8], id: &str) -> Result<String> {
    with_store(|store| {
        check(token, store)?;
        store
            .values
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("bitwarden secret get failed"))
    })
}
pub(crate) fn create_secret(
    token: &[u8],
    project: &str,
    name: &str,
    value: String,
) -> Result<String> {
    with_store(|store| {
        check(token, store)?;
        let id = format!("bws-secret-id-{name}");
        store
            .project_secrets
            .entry(project.into())
            .or_default()
            .insert(id.clone(), name.into());
        store.values.insert(id.clone(), value);
        Ok(id)
    })
}
pub(crate) fn replace_secret(
    token: &[u8],
    project: &str,
    id: &str,
    name: &str,
    value: String,
) -> Result<()> {
    with_store(|store| {
        check(token, store)?;
        let secrets = store
            .project_secrets
            .get_mut(project)
            .ok_or_else(|| anyhow::anyhow!("bitwarden project not found"))?;
        if !secrets.contains_key(id) || !store.values.contains_key(id) {
            anyhow::bail!("bitwarden secret get failed")
        }
        secrets.insert(id.into(), name.into());
        store.values.insert(id.into(), value);
        Ok(())
    })
}

pub(crate) async fn list_bws_projects(
    access_token: &ProtectedSecret,
) -> Result<Vec<BwsLookupCandidate<BwsProjectId>>> {
    Ok(list_projects(&access_token.to_test_bytes())?
        .into_iter()
        .map(|(id, name)| BwsLookupCandidate {
            id: BwsProjectId::new(id),
            name,
        })
        .collect())
}
pub(crate) async fn list_bws_secrets(
    access_token: &ProtectedSecret,
    project_id: &BwsProjectId,
) -> Result<Vec<BwsLookupCandidate<BwsSecretId>>> {
    Ok(
        list_project_secrets(&access_token.to_test_bytes(), project_id.as_str())?
            .into_iter()
            .map(|(id, name)| BwsLookupCandidate {
                id: BwsSecretId::new(id),
                name,
            })
            .collect(),
    )
}
pub(crate) async fn fetch_gpg_backup_envelope(
    access_token: &ProtectedSecret,
    secret_id: &BwsSecretId,
) -> Result<(String, BackupUpdateGuard)> {
    let value = read_secret(&access_token.to_test_bytes(), secret_id.as_str())?;
    Ok((
        value.clone(),
        BackupUpdateGuard::from_value_bytes(value.as_bytes()),
    ))
}
pub(crate) async fn fetch_password_store_remote(
    access_token: &ProtectedSecret,
    secret_id: &BwsSecretId,
) -> Result<String> {
    read_secret(&access_token.to_test_bytes(), secret_id.as_str())
}
pub(crate) async fn create_gpg_backup_envelope(
    access_token: &ProtectedSecret,
    project_id: &BwsProjectId,
    secret_key: &str,
    envelope: &GpgBackupEnvelope,
) -> Result<BwsSecretId> {
    let value = String::from_utf8(envelope.to_json()?)
        .map_err(|_| anyhow::anyhow!("gpg backup envelope is not valid UTF-8"))?;
    Ok(BwsSecretId::new(create_secret(
        &access_token.to_test_bytes(),
        project_id.as_str(),
        secret_key,
        value,
    )?))
}
pub(crate) async fn update_gpg_backup_envelope_if_unchanged(
    access_token: &ProtectedSecret,
    project_id: &BwsProjectId,
    secret_id: &BwsSecretId,
    secret_key: &str,
    envelope: &GpgBackupEnvelope,
    expected_guard: &BackupUpdateGuard,
) -> Result<()> {
    let token = access_token.to_test_bytes();
    let current = read_secret(&token, secret_id.as_str())?;
    expected_guard.ensure_matches(&BackupUpdateGuard::from_value_bytes(current.as_bytes()))?;
    let value = String::from_utf8(envelope.to_json()?)
        .map_err(|_| anyhow::anyhow!("gpg backup envelope is not valid UTF-8"))?;
    replace_secret(
        &token,
        project_id.as_str(),
        secret_id.as_str(),
        secret_key,
        value,
    )
}
pub(crate) async fn create_password_store_remote(
    access_token: &ProtectedSecret,
    project_id: &BwsProjectId,
    secret_key: &str,
    remote: &PasswordStoreRemote,
) -> Result<BwsSecretId> {
    Ok(BwsSecretId::new(create_secret(
        &access_token.to_test_bytes(),
        project_id.as_str(),
        secret_key,
        remote.as_str().to_owned(),
    )?))
}
pub(crate) async fn fetch_password_store_remote_guard(
    access_token: &ProtectedSecret,
    secret_id: &BwsSecretId,
) -> Result<BackupUpdateGuard> {
    let value = read_secret(&access_token.to_test_bytes(), secret_id.as_str())?;
    Ok(BackupUpdateGuard::from_value_bytes(value.as_bytes()))
}
pub(crate) async fn update_password_store_remote_if_unchanged(
    access_token: &ProtectedSecret,
    project_id: &BwsProjectId,
    secret_id: &BwsSecretId,
    secret_key: &str,
    remote: &PasswordStoreRemote,
    expected_guard: &BackupUpdateGuard,
) -> Result<()> {
    let token = access_token.to_test_bytes();
    let current = read_secret(&token, secret_id.as_str())?;
    expected_guard.ensure_matches(&BackupUpdateGuard::from_value_bytes(current.as_bytes()))?;
    replace_secret(
        &token,
        project_id.as_str(),
        secret_id.as_str(),
        secret_key,
        remote.as_str().to_owned(),
    )
}
