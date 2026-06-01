//! `secrets-internal-test-stub` feature 専用の Git clone / password-store filesystem の adapter backend stub。
//!
//! production build には compile されず、runtime flag ではなく compile-time feature selection で real
//! git2 / filesystem backend と差し替わる。integration test はこの module を import せず、同じ `dotfiles`
//! binary を実行する。
//!
//! この stub は Git port の datastore 境界だけを受け持つ。初期条件は
//! `secrets_internal_test_stub_contract::GIT_STUB_SPEC_ENV` の Git 専用 spec から private datastore へ
//! 展開し、最終観測 JSON は stdout の sentinel line として書き出す。YubiKey / BWS / GPG port stub とは
//! state/schema/file を共有しない。clone は実 Git/SSH を行わず、spec が与えた store 構成（`gpg_id_present`）
//! を「clone 後 store として観測される状態」へ反映するだけの datastore 遷移として模す。

use std::sync::{Mutex, OnceLock};

use anyhow::Context;

use crate::{
    Result,
    secrets::{
        domain::pass_restore::{PasswordStoreReadiness, PasswordStoreRemote},
        ports::git::{GitClonePort, PasswordStorePort},
    },
    secrets_internal_test_stub_contract::{GIT_STUB_SPEC_ENV, STUB_OBSERVATION_PREFIX},
};

#[derive(serde::Deserialize)]
struct GitStubSpec {
    /// clone 前に `~/.password-store` が既に存在するか。
    #[serde(default)]
    store_exists: bool,
    /// clone 後に store root へ `.gpg-id` が観測されるか（`pass` 可読性の模擬）。
    #[serde(default = "default_true")]
    gpg_id_present: bool,
}

fn default_true() -> bool {
    true
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct GitDatastore {
    store_exists: bool,
    gpg_id_present: bool,
    cloned_remotes: Vec<String>,
}

#[derive(serde::Serialize)]
struct GitObservation {
    cloned_remotes: Vec<String>,
    store_exists: bool,
}

#[derive(serde::Serialize)]
struct GitObservationFrame<'a> {
    port: &'static str,
    observation: &'a GitObservation,
}

static GIT_DATASTORE: OnceLock<Mutex<Option<GitDatastore>>> = OnceLock::new();

/// password-store filesystem port の internal backend stub。
#[derive(Default)]
pub(super) struct PasswordStoreStub;

/// Git clone port の internal backend stub。
#[derive(Default)]
pub(super) struct GitCloneStub;

impl PasswordStorePort for PasswordStoreStub {
    fn password_store_exists(&self) -> Result<bool> {
        with_datastore(|store| Ok(store.store_exists))
    }

    fn inspect_password_store(&self) -> Result<PasswordStoreReadiness> {
        with_datastore(|store| {
            Ok(PasswordStoreReadiness {
                gpg_id_present: store.gpg_id_present,
            })
        })
    }
}

impl GitClonePort for GitCloneStub {
    fn clone_password_store(&mut self, remote: &PasswordStoreRemote) -> Result<()> {
        with_datastore(|store| {
            // clone は実 Git/SSH を行わず、clone 後 store が存在する状態へ datastore を遷移させる。
            store.cloned_remotes.push(remote.as_str().to_owned());
            store.store_exists = true;
            Ok(())
        })
    }
}

fn with_datastore<T>(f: impl FnOnce(&mut GitDatastore) -> Result<T>) -> Result<T> {
    let datastore = GIT_DATASTORE.get_or_init(|| Mutex::new(None));
    let mut state = datastore
        .lock()
        .map_err(|_| anyhow::anyhow!("Git internal stub datastore lock is poisoned"))?;
    if state.is_none() {
        *state = Some(load_datastore()?);
    }
    let store = state
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("Git internal stub datastore is not initialized"))?;
    let out = f(store)?;
    write_observation(store)?;
    Ok(out)
}

fn load_datastore() -> Result<GitDatastore> {
    let body = std::env::var(GIT_STUB_SPEC_ENV)
        .context("Git internal stub spec JSON is not configured")?;
    let spec: GitStubSpec =
        serde_json::from_str(&body).context("failed to decode Git internal stub spec JSON")?;
    Ok(GitDatastore {
        store_exists: spec.store_exists,
        gpg_id_present: spec.gpg_id_present,
        cloned_remotes: Vec::new(),
    })
}

fn write_observation(store: &GitDatastore) -> Result<()> {
    let observation = GitObservation {
        cloned_remotes: store.cloned_remotes.clone(),
        store_exists: store.store_exists,
    };
    let frame = GitObservationFrame {
        port: "git",
        observation: &observation,
    };
    println!(
        "{STUB_OBSERVATION_PREFIX}{}",
        serde_json::to_string(&frame)?
    );
    Ok(())
}
