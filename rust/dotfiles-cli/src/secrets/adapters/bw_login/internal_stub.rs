//! `secrets-internal-test-stub` feature 専用の Bitwarden Password Manager CLI（`bw`）login adapter backend stub。
//!
//! production build には compile されず、runtime flag ではなく compile-time feature selection で実 `bw` CLI
//! backend と差し替わる。integration test はこの module を import せず、同じ `dotfiles` binary を実行する。
//!
//! この stub は bw-login port の外部 command 境界だけを受け持つ。初期条件は
//! `secrets_internal_test_stub_contract::BW_LOGIN_STUB_SPEC_ENV` の bw-login 専用 spec から読み、
//! login / unlock の成立可否と reachability を模す。最終観測 JSON は stdout の sentinel line として書き出す。
//! YubiKey / BWS / GPG / Git port stub とは state/schema/file を共有しない。実 `bw` CLI は起動しない。

use anyhow::Context;

use crate::{
    Result,
    secrets::{domain::bw_login::BwLoginSummary, support::protection::ProtectedSecret},
    secrets_internal_test_stub_contract::{BW_LOGIN_STUB_SPEC_ENV, STUB_OBSERVATION_PREFIX},
};

#[derive(serde::Deserialize)]
struct BwLoginStubSpec {
    /// `login_and_unlock` を成立させるか。`false` の場合は login 失敗を模して停止する。
    #[serde(default = "default_true")]
    login_succeeds: bool,
    /// `check_bw_login_reachable` を成立させるか。`false` の場合は到達失敗を模す。
    #[serde(default = "default_true")]
    reachable: bool,
}

fn default_true() -> bool {
    true
}

#[derive(serde::Serialize)]
struct BwLoginObservation {
    logged_in: bool,
    unlocked: bool,
}

#[derive(serde::Serialize)]
struct BwLoginObservationFrame<'a> {
    port: &'static str,
    observation: &'a BwLoginObservation,
}

/// `bw login` / `bw unlock` を実 CLI なしで模す。login 成立可否は spec で与える。
pub(super) fn login_and_unlock(
    _email: &ProtectedSecret,
    _password: &ProtectedSecret,
    _otp: &str,
) -> Result<BwLoginSummary> {
    let spec = load_spec()?;
    if !spec.login_succeeds {
        write_observation(false, false)?;
        anyhow::bail!("bw-login internal stub: configured login failure");
    }
    write_observation(true, true)?;
    Ok(BwLoginSummary::established())
}

/// `bw` CLI 到達確認を実 CLI なしで模す。到達可否は spec で与える。
pub(super) fn check_bw_login_reachable() -> Result<()> {
    let spec = load_spec()?;
    if !spec.reachable {
        anyhow::bail!("bw-login internal stub: configured reachability failure");
    }
    Ok(())
}

fn load_spec() -> Result<BwLoginStubSpec> {
    let body = std::env::var(BW_LOGIN_STUB_SPEC_ENV)
        .context("bw-login internal stub spec JSON is not configured")?;
    serde_json::from_str(&body).context("failed to decode bw-login internal stub spec JSON")
}

fn write_observation(logged_in: bool, unlocked: bool) -> Result<()> {
    let observation = BwLoginObservation {
        logged_in,
        unlocked,
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
