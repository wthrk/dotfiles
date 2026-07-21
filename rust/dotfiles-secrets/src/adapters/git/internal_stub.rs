//! `secrets-internal-test-stub` feature の Git port translation。

use crate::{
    Result,
    domain::pass_restore::{PasswordStoreReadiness, PasswordStoreRemote},
    ports::git::{GitClonePort, PasswordStorePort},
    support::{
        adapter_backend::{GitCloneBackend, PasswordStoreBackend},
        internal_stub_git,
    },
};

impl PasswordStorePort for PasswordStoreBackend {
    fn password_store_exists(&self) -> Result<bool> {
        internal_stub_git::store_exists()
    }

    fn inspect_password_store(&self) -> Result<PasswordStoreReadiness> {
        let (gpg_id_present, gpg_id_recipients, sample_entry) = internal_stub_git::inspection()?;
        Ok(PasswordStoreReadiness {
            gpg_id_present,
            gpg_id_recipients,
            sample_entry,
        })
    }
}

impl GitClonePort for GitCloneBackend {
    fn clone_password_store(&mut self, remote: &PasswordStoreRemote) -> Result<()> {
        internal_stub_git::record_clone(remote.as_str())
    }
}
