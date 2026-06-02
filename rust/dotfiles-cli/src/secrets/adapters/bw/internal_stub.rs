//! `secrets-internal-test-stub` feature 専用の BWS adapter backend stub。
//!
//! production build には compile されず、runtime flag ではなく compile-time feature selection で
//! real BWS SDK backend と差し替わる。integration test はこの module を import せず、同じ
//! `dotfiles` binary を実行する。
//!
//! この stub は BWS port の datastore 境界だけを受け持つ。初期条件は
//! `secrets_internal_test_stub_contract::BWS_STUB_SPEC_ENV` の BWS 専用 spec から private datastore
//! へ展開し、最終観測 JSON は stdout の sentinel line として書き出す。
//! YubiKey port stub とは state/schema/file を共有しない。

use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};

use anyhow::Context;

use crate::secrets::{
    domain::{
        bws::{BwsLookupCandidate, BwsProjectId, BwsSecretId},
        gpg_backup::{BackupUpdateGuard, GpgBackupEnvelope},
        pass_restore::PasswordStoreRemote,
    },
    ports::bw::BwsClientPort,
    support::protection::ProtectedSecret,
};
use crate::secrets_internal_test_stub_contract::{BWS_STUB_SPEC_ENV, STUB_OBSERVATION_PREFIX};

#[derive(serde::Deserialize)]
struct BwsStubSpec {
    fixture: BwsFixture,
    /// `gpg-secret-key-backup` secret value を override する任意の encrypted envelope JSON。
    ///
    /// restore-gpg / spare 追加の integration test が、stub recipient と整合した envelope を初期 datastore
    /// として投入するために使う。未指定時は fixture 既定の "gpg-secret" 値を維持する。
    #[serde(default)]
    gpg_secret_key_backup: Option<String>,
    /// `password-store-remote` secret value を override する任意の clone URL。
    ///
    /// restore-pass の integration test が、domain で妥当な `git@github.com:<owner>/<repo>.git` を初期
    /// datastore として投入するために使う。未指定時は fixture 既定値を維持する。
    #[serde(default)]
    password_store_remote: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum BwsFixture {
    DefaultRecoveryProject,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct BwsDatastore {
    projects: BTreeMap<String, String>,
    project_secrets: BTreeMap<String, BTreeMap<String, String>>,
    secret_values: BTreeMap<String, String>,
}

#[derive(serde::Serialize)]
struct BwsObservation {
    resolved_secrets: BTreeMap<String, String>,
}

#[derive(serde::Serialize)]
struct BwsObservationFrame<'a> {
    port: &'static str,
    observation: &'a BwsObservation,
}

static BWS_DATASTORE: OnceLock<Mutex<Option<BwsDatastore>>> = OnceLock::new();

impl BwsClientPort for super::BwsClientAdapter {
    async fn list_bws_projects(
        &self,
        access_token: &ProtectedSecret,
    ) -> crate::Result<Vec<BwsLookupCandidate<BwsProjectId>>> {
        read_bws_projects(access_token)
    }

    async fn list_bws_secrets(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
    ) -> crate::Result<Vec<BwsLookupCandidate<BwsSecretId>>> {
        read_bws_secrets(access_token, project_id)
    }

    async fn fetch_bws_secret_by_id(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<ProtectedSecret> {
        read_bws_secret_by_id(access_token, secret_id)
    }

    async fn fetch_gpg_backup_envelope(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<(GpgBackupEnvelope, BackupUpdateGuard)> {
        with_datastore(|store| {
            ensure_access_token_matches_datastore(access_token, store)?;
            let value = store
                .secret_values
                .get(secret_id.as_str())
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("bitwarden secret get failed"))?;
            let envelope = GpgBackupEnvelope::from_json(value.as_bytes())?;
            let guard = BackupUpdateGuard::from_value_bytes(value.as_bytes());
            Ok((envelope, guard))
        })
    }

    async fn fetch_password_store_remote(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<PasswordStoreRemote> {
        with_datastore(|store| {
            ensure_access_token_matches_datastore(access_token, store)?;
            let value = store
                .secret_values
                .get(secret_id.as_str())
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("bitwarden secret get failed"))?;
            PasswordStoreRemote::parse(&value)
        })
    }

    async fn create_gpg_backup_envelope(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
        key: &str,
        envelope: &GpgBackupEnvelope,
    ) -> crate::Result<BwsSecretId> {
        let value = String::from_utf8(envelope.to_json()?)
            .map_err(|_| anyhow::anyhow!("gpg backup envelope is not valid UTF-8"))?;
        with_datastore(|store| {
            ensure_access_token_matches_datastore(access_token, store)?;
            let secret_id = format!("bws-secret-id-{key}");
            store
                .project_secrets
                .entry(project_id.as_str().to_owned())
                .or_default()
                .insert(secret_id.clone(), key.to_owned());
            store.secret_values.insert(secret_id.clone(), value);
            Ok(BwsSecretId::new(secret_id))
        })
    }

    async fn update_gpg_backup_envelope_if_unchanged(
        &self,
        access_token: &ProtectedSecret,
        _project_id: &BwsProjectId,
        secret_id: &BwsSecretId,
        _key: &str,
        envelope: &GpgBackupEnvelope,
        expected_guard: &BackupUpdateGuard,
    ) -> crate::Result<()> {
        let value = String::from_utf8(envelope.to_json()?)
            .map_err(|_| anyhow::anyhow!("gpg backup envelope is not valid UTF-8"))?;
        with_datastore(|store| {
            ensure_access_token_matches_datastore(access_token, store)?;
            let current = store
                .secret_values
                .get(secret_id.as_str())
                .ok_or_else(|| anyhow::anyhow!("bitwarden secret get failed"))?;
            let current_guard = BackupUpdateGuard::from_value_bytes(current.as_bytes());
            expected_guard.ensure_matches(&current_guard)?;
            store
                .secret_values
                .insert(secret_id.as_str().to_owned(), value);
            Ok(())
        })
    }

    async fn create_password_store_remote(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
        key: &str,
        remote: &PasswordStoreRemote,
    ) -> crate::Result<BwsSecretId> {
        // application が domain rule で検証済みの clone URL をそのまま datastore へ保存する。
        with_datastore(|store| {
            ensure_access_token_matches_datastore(access_token, store)?;
            let secret_id = format!("bws-secret-id-{key}");
            store
                .project_secrets
                .entry(project_id.as_str().to_owned())
                .or_default()
                .insert(secret_id.clone(), key.to_owned());
            store
                .secret_values
                .insert(secret_id.clone(), remote.as_str().to_owned());
            Ok(BwsSecretId::new(secret_id))
        })
    }

    async fn fetch_password_store_remote_guard(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<BackupUpdateGuard> {
        with_datastore(|store| {
            ensure_access_token_matches_datastore(access_token, store)?;
            let value = store
                .secret_values
                .get(secret_id.as_str())
                .ok_or_else(|| anyhow::anyhow!("bitwarden secret get failed"))?;
            Ok(BackupUpdateGuard::from_value_bytes(value.as_bytes()))
        })
    }

    async fn update_password_store_remote_if_unchanged(
        &self,
        access_token: &ProtectedSecret,
        _project_id: &BwsProjectId,
        secret_id: &BwsSecretId,
        _key: &str,
        remote: &PasswordStoreRemote,
        expected_guard: &BackupUpdateGuard,
    ) -> crate::Result<()> {
        with_datastore(|store| {
            ensure_access_token_matches_datastore(access_token, store)?;
            let current = store
                .secret_values
                .get(secret_id.as_str())
                .ok_or_else(|| anyhow::anyhow!("bitwarden secret get failed"))?;
            let current_guard = BackupUpdateGuard::from_value_bytes(current.as_bytes());
            expected_guard.ensure_matches(&current_guard)?;
            store
                .secret_values
                .insert(secret_id.as_str().to_owned(), remote.as_str().to_owned());
            Ok(())
        })
    }
}

fn read_bws_projects(
    access_token: &ProtectedSecret,
) -> crate::Result<Vec<BwsLookupCandidate<BwsProjectId>>> {
    with_datastore(|store| {
        ensure_access_token_matches_datastore(access_token, store)?;
        Ok(store
            .projects
            .iter()
            .map(|(project_id, project_name)| BwsLookupCandidate {
                id: BwsProjectId::new(project_id.clone()),
                name: project_name.clone(),
            })
            .collect())
    })
}

fn read_bws_secrets(
    access_token: &ProtectedSecret,
    project_id: &BwsProjectId,
) -> crate::Result<Vec<BwsLookupCandidate<BwsSecretId>>> {
    with_datastore(|store| {
        ensure_access_token_matches_datastore(access_token, store)?;
        let candidates = store
            .project_secrets
            .get(project_id.as_str())
            .ok_or_else(|| anyhow::anyhow!("bitwarden project not found"))?;
        Ok(candidates
            .iter()
            .map(|(secret_id, secret_name)| BwsLookupCandidate {
                id: BwsSecretId::new(secret_id.clone()),
                name: secret_name.clone(),
            })
            .collect())
    })
}

fn read_bws_secret_by_id(
    access_token: &ProtectedSecret,
    secret_id: &BwsSecretId,
) -> crate::Result<ProtectedSecret> {
    with_datastore(|store| {
        ensure_access_token_matches_datastore(access_token, store)?;
        let value = store
            .secret_values
            .get(secret_id.as_str())
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("bitwarden secret get failed"))?;
        protected_secret_from_string(value)
    })
}

fn ensure_access_token_matches_datastore(
    access_token: &ProtectedSecret,
    store: &BwsDatastore,
) -> crate::Result<()> {
    let configured = store
        .secret_values
        .get("bws-secret-id-access-token")
        .ok_or_else(|| anyhow::anyhow!("bws access token stub secret is not configured"))?;
    if access_token.to_test_bytes() == configured.as_bytes() {
        Ok(())
    } else {
        anyhow::bail!("bitwarden login failed")
    }
}

fn protected_secret_from_string(value: String) -> crate::Result<ProtectedSecret> {
    let session = crate::secrets::support::protection::SecretSession::start()?;
    let buffer = crate::secrets::support::protection::buffer::ProtectedInputBuffer::read_line_from(
        std::io::Cursor::new(value.into_bytes()),
        16 * 1024,
        &session,
    )?;
    buffer.into_protected_secret_line(&session, 16 * 1024, "internal stub secret is too large")
}

fn with_datastore<T>(f: impl FnOnce(&mut BwsDatastore) -> crate::Result<T>) -> crate::Result<T> {
    let datastore = BWS_DATASTORE.get_or_init(|| Mutex::new(None));
    let mut state = datastore
        .lock()
        .map_err(|_| anyhow::anyhow!("BWS internal stub datastore lock is poisoned"))?;
    if state.is_none() {
        *state = Some(load_datastore()?);
    }
    let store = state
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("BWS internal stub datastore is not initialized"))?;
    let out = f(store)?;
    write_observation(store)?;
    Ok(out)
}

fn load_datastore() -> crate::Result<BwsDatastore> {
    let body = std::env::var(BWS_STUB_SPEC_ENV)
        .context("BWS internal stub spec JSON is not configured")?;
    let spec: BwsStubSpec =
        serde_json::from_str(&body).context("failed to decode BWS internal stub spec JSON")?;
    Ok(datastore_from_spec(spec))
}

fn write_observation(store: &BwsDatastore) -> crate::Result<()> {
    let observation = observation_from_datastore(store);
    let frame = BwsObservationFrame {
        port: "bws",
        observation: &observation,
    };
    println!(
        "{STUB_OBSERVATION_PREFIX}{}",
        serde_json::to_string(&frame)?
    );
    Ok(())
}

fn datastore_from_spec(spec: BwsStubSpec) -> BwsDatastore {
    let mut datastore = match spec.fixture {
        BwsFixture::DefaultRecoveryProject => default_recovery_project_datastore(),
    };
    if let Some(envelope) = spec.gpg_secret_key_backup {
        datastore
            .secret_values
            .insert("bws-secret-id-gpg".to_owned(), envelope);
    }
    if let Some(remote) = spec.password_store_remote {
        datastore
            .secret_values
            .insert("bws-secret-id-pass".to_owned(), remote);
    }
    datastore
}

fn default_recovery_project_datastore() -> BwsDatastore {
    let mut projects = BTreeMap::new();
    projects.insert(
        "bws-project-id-dotfiles".to_owned(),
        "dotfiles-secret-recovery".to_owned(),
    );

    let mut recovery_secrets = BTreeMap::new();
    recovery_secrets.insert(
        "bws-secret-id-gpg".to_owned(),
        "gpg-secret-key-backup".to_owned(),
    );
    recovery_secrets.insert(
        "bws-secret-id-pass".to_owned(),
        "password-store-remote".to_owned(),
    );

    let mut project_secrets = BTreeMap::new();
    project_secrets.insert("bws-project-id-dotfiles".to_owned(), recovery_secrets);

    let mut secret_values = BTreeMap::new();
    secret_values.insert("bws-secret-id-access-token".to_owned(), "token".to_owned());
    secret_values.insert("bws-secret-id-gpg".to_owned(), "gpg-secret".to_owned());
    secret_values.insert(
        "bws-secret-id-pass".to_owned(),
        "https://example.invalid/repo.git".to_owned(),
    );

    BwsDatastore {
        projects,
        project_secrets,
        secret_values,
    }
}

fn observation_from_datastore(store: &BwsDatastore) -> BwsObservation {
    let mut resolved_secrets = BTreeMap::new();
    for project_secrets in store.project_secrets.values() {
        for (secret_id, secret_name) in project_secrets {
            if let Some(value) = store.secret_values.get(secret_id) {
                resolved_secrets.insert(secret_name.clone(), value.clone());
            }
        }
    }
    BwsObservation { resolved_secrets }
}
