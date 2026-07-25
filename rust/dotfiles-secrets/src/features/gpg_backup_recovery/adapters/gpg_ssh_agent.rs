//! gpg-agent SSH port の forwarding-only adapter。
use crate::{
    Result,
    features::gpg_backup_recovery::domain::gpg_restore::{
        Keygrip, OpenSshPublicKey, SshAgentReadiness,
    },
    features::gpg_backup_recovery::ports::gpg::SshAgentPort,
    features::gpg_backup_recovery::support::backend::SshAgentBackend,
    features::gpg_backup_recovery::support::ssh_agent_backend,
};
impl SshAgentPort for SshAgentBackend {
    fn register_authentication_subkey(&mut self, keygrip: &Keygrip) -> Result<bool> {
        ssh_agent_backend::register_authentication_subkey(keygrip)
    }
    fn unregister_authentication_subkey(&mut self, keygrip: &Keygrip) -> Result<()> {
        ssh_agent_backend::unregister_authentication_subkey(keygrip)
    }
    fn inspect_ssh_agent(&mut self, expected: &OpenSshPublicKey) -> Result<SshAgentReadiness> {
        ssh_agent_backend::inspect_ssh_agent(expected)
    }
}

#[cfg(all(not(test), not(feature = "secrets-internal-test-stub")))]
impl crate::features::gpg_backup_recovery::ports::gpg::GpgAgentSocketPort
    for crate::features::gpg_backup_recovery::support::backend::GpgAgentSocketBackend
{
    fn resolve_strict_socket(&mut self) -> Result<Option<std::path::PathBuf>> {
        ssh_agent_backend::resolve_gpg_agent_socket()
    }
}

#[cfg(all(test, not(feature = "secrets-internal-test-stub")))]
impl crate::features::gpg_backup_recovery::ports::gpg::GpgAgentSocketPort
    for crate::features::gpg_backup_recovery::support::backend::GpgAgentSocketBackend
{
}
