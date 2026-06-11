//! `secrets-internal-test-stub` feature 専用の Bitwarden 個人 vault adapter backend stub。
//!
//! この module は production build に混入せず、compile-time feature selection で実 backend と排他的に
//! 差し替わる。runtime の real/stub 分岐は作らず、integration test は production command path を実行した
//! うえで test-only stdout sentinel から最終 datastore だけを観測する。stdout observation は
//! `secrets-internal-test-stub` build 専用の明示観測境界であり、本物 secret の出力経路ではない。

use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};

use anyhow::Context;

use crate::secrets::{
    domain::{
        gpg_backup::GpgBackupEnvelope,
        pass_restore::PasswordStoreRemote,
        vault::{BitwardenVaultCredentials, VaultLookupCandidate, VaultSecretId, VaultSecretName},
    },
    ports::bw::VaultClientPort,
};
use crate::secrets_internal_test_stub_contract::{STUB_OBSERVATION_PREFIX, VAULT_STUB_SPEC_ENV};

#[derive(serde::Deserialize)]
struct VaultStubSpec {
    #[serde(default)]
    secrets: BTreeMap<String, String>,
    #[serde(default)]
    auth_fails: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct VaultDatastore {
    secrets: BTreeMap<String, VaultSecretRecord>,
    auth_fails: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct VaultSecretRecord {
    id: String,
    name: String,
    value: String,
}

#[derive(serde::Serialize)]
struct VaultObservation {
    secrets: BTreeMap<String, String>,
}

#[derive(serde::Serialize)]
struct VaultObservationFrame<'a> {
    port: &'static str,
    observation: &'a VaultObservation,
}

static VAULT_DATASTORE: OnceLock<Mutex<Option<VaultDatastore>>> = OnceLock::new();

#[derive(Debug)]
struct VaultStubDatastoreLockPoisoned {
    source: DatastoreLockPoisonSource,
}

impl VaultStubDatastoreLockPoisoned {
    fn from_poison<T>(source: std::sync::PoisonError<T>) -> Self {
        Self {
            source: DatastoreLockPoisonSource::from_poison(source),
        }
    }
}

impl std::fmt::Display for VaultStubDatastoreLockPoisoned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Bitwarden vault internal stub datastore lock is poisoned"
        )
    }
}

impl std::error::Error for VaultStubDatastoreLockPoisoned {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[derive(Debug)]
struct DatastoreLockPoisonSource {
    message: String,
}

impl DatastoreLockPoisonSource {
    fn from_poison<T>(source: std::sync::PoisonError<T>) -> Self {
        Self {
            message: source.to_string(),
        }
    }
}

impl std::fmt::Display for DatastoreLockPoisonSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DatastoreLockPoisonSource {}

impl VaultClientPort for super::VaultClientAdapter {
    async fn list_vault_secrets(
        &self,
        credentials: &BitwardenVaultCredentials,
    ) -> crate::Result<Vec<VaultLookupCandidate<VaultSecretId>>> {
        ensure_auth(credentials)?;
        with_datastore(|store| {
            Ok(store
                .secrets
                .values()
                .map(|secret| VaultLookupCandidate {
                    id: VaultSecretId::new(secret.id.clone()),
                    name: secret.name.clone(),
                })
                .collect())
        })
    }

    async fn fetch_gpg_backup_envelope(
        &self,
        credentials: &BitwardenVaultCredentials,
        secret_id: &VaultSecretId,
    ) -> crate::Result<GpgBackupEnvelope> {
        ensure_auth(credentials)?;
        let value = read_secret_value(secret_id)?;
        GpgBackupEnvelope::from_json(value.as_bytes())
    }

    async fn fetch_password_store_remote(
        &self,
        credentials: &BitwardenVaultCredentials,
        secret_id: &VaultSecretId,
    ) -> crate::Result<PasswordStoreRemote> {
        ensure_auth(credentials)?;
        PasswordStoreRemote::parse(read_secret_value(secret_id)?.as_str())
    }

    async fn create_password_store_remote(
        &self,
        credentials: &BitwardenVaultCredentials,
        remote: &PasswordStoreRemote,
    ) -> crate::Result<VaultSecretId> {
        ensure_auth(credentials)?;
        with_datastore(|store| {
            let id = "password-store-remote-created".to_owned();
            store.secrets.insert(
                id.clone(),
                VaultSecretRecord {
                    id: id.clone(),
                    name: VaultSecretName::PasswordStoreRemote.key().to_owned(),
                    value: remote.as_str().to_owned(),
                },
            );
            Ok(VaultSecretId::new(id))
        })
    }
}

fn ensure_auth(credentials: &BitwardenVaultCredentials) -> crate::Result<()> {
    let _ = credentials.api_key().client_id().len();
    let _ = credentials.api_key().client_secret().len();
    let _ = credentials.master_password().len();
    with_datastore(|store| {
        if store.auth_fails {
            anyhow::bail!("Bitwarden vault internal stub rejected the provided account API key");
        }
        Ok(())
    })
}

fn read_secret_value(secret_id: &VaultSecretId) -> crate::Result<String> {
    with_datastore(|store| {
        store
            .secrets
            .get(secret_id.as_str())
            .map(|secret| secret.value.clone())
            .ok_or_else(|| anyhow::anyhow!("Bitwarden vault internal stub secret not found"))
    })
}

fn datastore() -> &'static Mutex<Option<VaultDatastore>> {
    VAULT_DATASTORE.get_or_init(|| Mutex::new(None))
}

fn with_datastore<T>(
    operation: impl FnOnce(&mut VaultDatastore) -> crate::Result<T>,
) -> crate::Result<T> {
    let mut guard = datastore()
        .lock()
        .map_err(VaultStubDatastoreLockPoisoned::from_poison)?;
    if guard.is_none() {
        *guard = Some(load_datastore()?);
    }
    let store = guard.as_mut().expect("datastore initialized");
    let result = operation(store)?;
    emit_observation(store)?;
    Ok(result)
}

fn load_datastore() -> crate::Result<VaultDatastore> {
    let Some(raw) = std::env::var_os(VAULT_STUB_SPEC_ENV) else {
        return Ok(VaultDatastore::default());
    };
    let spec: VaultStubSpec = serde_json::from_str(&raw.to_string_lossy())
        .context("failed to decode Bitwarden vault internal stub spec")?;
    Ok(VaultDatastore {
        secrets: spec
            .secrets
            .into_iter()
            .map(|(name, value)| {
                let id = format!("{name}-id");
                (id.clone(), VaultSecretRecord { id, name, value })
            })
            .collect(),
        auth_fails: spec.auth_fails,
    })
}

fn emit_observation(store: &VaultDatastore) -> crate::Result<()> {
    let observation = VaultObservation {
        secrets: store
            .secrets
            .values()
            .map(|secret| (secret.name.clone(), "<redacted>".to_owned()))
            .collect(),
    };
    println!(
        "{}{}",
        STUB_OBSERVATION_PREFIX,
        serde_json::to_string(&VaultObservationFrame {
            port: "bitwarden-vault",
            observation: &observation,
        })?
    );
    Ok(())
}
