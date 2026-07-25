//! gpg-agent SSH port の forwarding-only adapter。
use crate::{
    Result,
    features::gpg_backup_recovery::domain::gpg_restore::{
        Keygrip, OpenSshPublicKey, SshAgentReadiness,
    },
    features::gpg_backup_recovery::ports::gpg::SshAgentPort,
    features::gpg_backup_recovery::support::ssh_agent_backend,
    shared::contracts::adapter_backend::SshAgentBackend,
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
