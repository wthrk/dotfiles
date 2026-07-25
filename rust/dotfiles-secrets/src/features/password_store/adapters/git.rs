//! private `password-store` repository の Git clone と store filesystem 観測を port 契約へ接続する adapter 群。
//!
//! production build（`gpg-backend`）では git2 + libssh2 の SSH agent 認証 clone と、`~/.password-store` の
//! filesystem 観測へ接続する。`secrets-internal-test-stub` feature では同じ port 契約を満たす internal
//! backend stub と compile-time で差し替え、runtime real/stub 分岐は作らない。integration test は stub
//! module を import せず、Cargo が `secrets-internal-test-stub` feature 付きで事前に build した専用
//! `dotfiles-secrets-internal-test-stub` binary を実行する。この binary は通常 CLI と同じ
//! `dotfiles_cli::dispatch` entrypoint を呼ぶ。専用 target は `required-features` で featureless な通常
//! artifact と分離されるため、Git/SSH backend は internal stub に固定され、実 Git/SSH backend へ runtime に
//! fallback しない。SSH agent 経路は #14 の `ssh_agent_adapter` と同じ socket 解決規則を流用し、`git` CLI と
//! GitHub API は使わない。

#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
use crate::{
    Result,
    features::password_store::domain::pass_restore::{PasswordStoreReadiness, PasswordStoreRemote},
    features::password_store::ports::git::{GitClonePort, PasswordStorePort},
    features::password_store::support::{git_clone, password_store},
    shared::contracts::adapter_backend::{GitCloneBackend, PasswordStoreBackend},
};

#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
impl PasswordStorePort for PasswordStoreBackend {
    fn password_store_exists(&self) -> Result<bool> {
        password_store::password_store_exists()
    }

    fn inspect_password_store(&self) -> Result<PasswordStoreReadiness> {
        password_store::inspect_password_store()
    }
}

#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
impl GitClonePort for GitCloneBackend {
    fn clone_password_store(&mut self, remote: &PasswordStoreRemote) -> Result<()> {
        git_clone::clone_password_store(remote)
    }
}
