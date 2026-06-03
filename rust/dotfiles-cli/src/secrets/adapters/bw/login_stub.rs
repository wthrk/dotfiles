//! `secrets-internal-test-stub` feature 専用の bw-login adapter backend stub。
//!
//! production build には compile されず、runtime flag ではなく compile-time feature selection で real `bw`
//! CLI backend と差し替わる。integration test はこの module を import せず、同じ `dotfiles` binary を実行し、
//! bw-login port 専用の初期条件 spec JSON と最終状態観測 JSON だけを外部観測面として扱う。
//!
//! この stub は `bw` CLI を起動せず、login / unlock を datastore 遷移として模す。expected master password と
//! 一致しなければ login 失敗を返し、一致すれば設定済み session key を返す。observation には login で観測した
//! email / OTP（非秘匿 argv 値）と unlock 成否だけを書き、master password は決して観測へ出さない。
//! YubiKey / BWS / GPG / Git port stub とは state/schema/file を共有しない。

use std::sync::{Mutex, OnceLock};

use anyhow::Context;

use crate::secrets::{
    domain::bw_login::{BwLoginEmail, BwOtp, BwSessionKey},
    ports::bw::BwLoginPort,
    support::protection::ProtectedSecret,
};
use crate::secrets_internal_test_stub_contract::{BW_LOGIN_STUB_SPEC_ENV, STUB_OBSERVATION_PREFIX};

#[derive(serde::Deserialize)]
struct BwLoginStubSpec {
    /// login が成功する条件となる expected master password 平文。
    expected_password: String,
    /// unlock 成功時に返す `BW_SESSION` 値。
    session_key: String,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct BwLoginDatastore {
    expected_password: String,
    session_key: String,
    observed_email: Option<String>,
    observed_otp: Option<String>,
    unlocked: bool,
}

#[derive(serde::Serialize)]
struct BwLoginObservation {
    observed_email: Option<String>,
    observed_otp: Option<String>,
    unlocked: bool,
}

#[derive(serde::Serialize)]
struct BwLoginObservationFrame<'a> {
    port: &'static str,
    observation: &'a BwLoginObservation,
}

static BW_LOGIN_DATASTORE: OnceLock<Mutex<Option<BwLoginDatastore>>> = OnceLock::new();

impl BwLoginPort for super::BwLoginAdapter {
    async fn login_and_unlock(
        &self,
        email: &BwLoginEmail,
        password: &ProtectedSecret,
        otp: &BwOtp,
    ) -> crate::Result<BwSessionKey> {
        with_datastore(|store| {
            // master password を子プロセスへ渡す代わりに、expected 値との一致だけを stub の login 成否とする。
            // 比較用に取り出した平文 bytes は `Zeroizing` で包み、drop 時に process メモリから確実に消去する。
            let observed = zeroize::Zeroizing::new(password.to_test_bytes());
            if observed.as_slice() != store.expected_password.as_bytes() {
                anyhow::bail!("`bw login` failed; the master password did not match the stub spec");
            }
            store.observed_email = Some(email.as_str().to_owned());
            store.observed_otp = Some(otp.as_str().to_owned());
            store.unlocked = true;
            BwSessionKey::parse(&store.session_key)
        })
    }
}

fn with_datastore<T>(
    f: impl FnOnce(&mut BwLoginDatastore) -> crate::Result<T>,
) -> crate::Result<T> {
    let datastore = BW_LOGIN_DATASTORE.get_or_init(|| Mutex::new(None));
    let mut state = datastore
        .lock()
        .map_err(|_| anyhow::anyhow!("bw-login internal stub datastore lock is poisoned"))?;
    if state.is_none() {
        *state = Some(load_datastore()?);
    }
    let store = state
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("bw-login internal stub datastore is not initialized"))?;
    let out = f(store);
    write_observation(store)?;
    out
}

fn load_datastore() -> crate::Result<BwLoginDatastore> {
    let body = std::env::var(BW_LOGIN_STUB_SPEC_ENV)
        .context("bw-login internal stub spec JSON is not configured")?;
    let spec: BwLoginStubSpec =
        serde_json::from_str(&body).context("failed to decode bw-login internal stub spec JSON")?;
    Ok(BwLoginDatastore {
        expected_password: spec.expected_password,
        session_key: spec.session_key,
        observed_email: None,
        observed_otp: None,
        unlocked: false,
    })
}

fn write_observation(store: &BwLoginDatastore) -> crate::Result<()> {
    let observation = BwLoginObservation {
        observed_email: store.observed_email.clone(),
        observed_otp: store.observed_otp.clone(),
        unlocked: store.unlocked,
    };
    let frame = BwLoginObservationFrame {
        port: "bw-login",
        observation: &observation,
    };
    println!(
        "{STUB_OBSERVATION_PREFIX}{}",
        serde_json::to_string(&frame)?
    );
    Ok(())
}
