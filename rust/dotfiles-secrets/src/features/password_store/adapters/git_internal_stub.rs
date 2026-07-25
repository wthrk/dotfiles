//! `secrets-internal-test-stub` Git port の forwarding-only adapter。

use crate::{
    Result,
    features::password_store::domain::pass_restore::{PasswordStoreReadiness, PasswordStoreRemote},
    features::password_store::ports::git::{GitClonePort, PasswordStorePort},
    features::password_store::support::backend::{GitCloneBackend, PasswordStoreBackend},
    features::password_store::support::internal_stub_git,
};

impl PasswordStorePort for PasswordStoreBackend {
    fn password_store_exists(&self) -> Result<bool> {
        internal_stub_git::password_store_exists()
    }
    fn inspect_password_store(&self) -> Result<PasswordStoreReadiness> {
        internal_stub_git::inspect_password_store()
    }
}
impl GitClonePort for GitCloneBackend {
    fn clone_password_store(&mut self, remote: &PasswordStoreRemote) -> Result<()> {
        internal_stub_git::clone_password_store(remote)
    }
}
