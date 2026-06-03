//! `BwLoginPort` を Bitwarden Password Manager CLI（`bw`）login / unlock 境界へ接続する adapter。
//!
//! application は YubiKey 由来 secret の取得順序と OTP 入力順序を保持する。adapter は `bw login` /
//! `bw unlock` の外部 command 翻訳だけを担い、master password の借用と子プロセスへの env 受け渡しは
//! secret 保護境界（`support/protection/bw_login`）へ閉じる。`bw` CLI と OTP は復旧本線の login / unlock
//! 用途に限定し、secret 取得や永続保存用途で `bw` CLI は使わない（spec L190）。
//!
//! production build（`gpg-backend`）では実 `bw` CLI を起動し、`secrets-internal-test-stub` feature では
//! 同じ port 契約を満たす internal backend stub と compile-time で差し替え、runtime real/stub 分岐は作らない。
//! integration test は stub module を import せず、feature 有効でビルドされた同じ `dotfiles` binary を実行する。

#[cfg(feature = "secrets-internal-test-stub")]
mod internal_stub;

use crate::{
    Result,
    secrets::{
        domain::bw_login::BwLoginSummary, ports::bw_login::BwLoginPort,
        support::protection::ProtectedSecret,
    },
};

/// Bitwarden Password Manager CLI を `BwLoginPort` へ翻訳する adapter。
#[derive(Default)]
pub(in crate::secrets) struct BwLoginAdapter;

#[cfg(not(feature = "secrets-internal-test-stub"))]
impl BwLoginPort for BwLoginAdapter {
    fn login_and_unlock(
        &self,
        email: &ProtectedSecret,
        password: &ProtectedSecret,
        otp: &str,
    ) -> Result<BwLoginSummary> {
        crate::secrets::support::protection::bw_login::login_and_unlock(email, password, otp)?;
        Ok(BwLoginSummary::established())
    }

    fn check_bw_login_reachable(&self) -> Result<()> {
        crate::secrets::support::protection::bw_login::check_reachable()
    }
}

#[cfg(feature = "secrets-internal-test-stub")]
impl BwLoginPort for BwLoginAdapter {
    fn login_and_unlock(
        &self,
        email: &ProtectedSecret,
        password: &ProtectedSecret,
        otp: &str,
    ) -> Result<BwLoginSummary> {
        internal_stub::login_and_unlock(email, password, otp)
    }

    fn check_bw_login_reachable(&self) -> Result<()> {
        internal_stub::check_bw_login_reachable()
    }
}
