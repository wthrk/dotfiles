//! private `password-store` repository の Git clone と store filesystem 観測を port 契約へ接続する adapter 群。
//!
//! production build（`gpg-backend`）では git2 + libssh2 の SSH agent 認証 clone と、`~/.password-store` の
//! filesystem 観測へ接続する。`secrets-internal-test-stub` feature では同じ port 契約を満たす internal
//! backend stub と compile-time で差し替え、runtime real/stub 分岐は作らない。integration test は stub
//! module を import せず、feature 有効でビルドされた同じ `dotfiles` binary を実行する。SSH agent 経路は
//! #14 の `ssh_agent_adapter` と同じ socket 解決規則を流用し、`git` CLI と GitHub API は使わない。

#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
mod clone_adapter;
#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
mod store_adapter;

#[cfg(feature = "secrets-internal-test-stub")]
mod internal_stub;

use crate::{
    Result,
    secrets::{
        domain::pass_restore::{PasswordStoreReadiness, PasswordStoreRemote},
        ports::git::{GitClonePort, PasswordStorePort},
    },
};

/// password-store filesystem backend（real filesystem / internal stub）を `PasswordStorePort` へ翻訳する adapter。
#[derive(Default)]
pub(in crate::secrets) struct PasswordStoreAdapter(PasswordStoreInner);

#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
type PasswordStoreInner = store_adapter::PasswordStoreAdapter;
#[cfg(feature = "secrets-internal-test-stub")]
type PasswordStoreInner = internal_stub::PasswordStoreStub;

impl PasswordStorePort for PasswordStoreAdapter {
    fn password_store_exists(&self) -> Result<bool> {
        self.0.password_store_exists()
    }

    fn inspect_password_store(&self) -> Result<PasswordStoreReadiness> {
        self.0.inspect_password_store()
    }

    fn remove_password_store(&mut self) -> Result<()> {
        self.0.remove_password_store()
    }
}

/// Git clone backend（git2 + libssh2 / internal stub）を `GitClonePort` 契約へ翻訳する adapter。
#[derive(Default)]
pub(in crate::secrets) struct GitCloneAdapter(GitCloneInner);

#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
type GitCloneInner = clone_adapter::GitCloneAdapter;
#[cfg(feature = "secrets-internal-test-stub")]
type GitCloneInner = internal_stub::GitCloneStub;

impl GitClonePort for GitCloneAdapter {
    fn clone_password_store(&mut self, remote: &PasswordStoreRemote) -> Result<()> {
        self.0.clone_password_store(remote)
    }
}

/// clone 先と filesystem 観測対象の `~/.password-store` path を `$HOME` から解決する。
///
/// 設計（spec L174）は store path を `~/.password-store` に固定する。clone adapter と store adapter が
/// 同一 path を観測するための共有 path 解決であり、business 判断は持たない filesystem primitive である。
#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
fn password_store_path() -> Result<std::path::PathBuf> {
    use anyhow::Context;
    let home =
        std::env::var_os("HOME").context("HOME is not set; cannot resolve ~/.password-store")?;
    Ok(std::path::PathBuf::from(home)
        .join(crate::secrets::domain::pass_restore::PASSWORD_STORE_DIR_NAME))
}
