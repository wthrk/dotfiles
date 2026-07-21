//! gpg-agent SSH port の forwarding-only adapter。
use crate::{
    Result,
    domain::gpg_restore::{Keygrip, OpenSshPublicKey, SshAgentReadiness},
    ports::gpg::SshAgentPort,
    support::{adapter_backend::SshAgentBackend, ssh_agent_backend},
};
impl SshAgentPort for SshAgentBackend {
    fn register_authentication_subkey(&mut self, keygrip: &Keygrip) -> Result<()> {
        ssh_agent_backend::register_authentication_subkey(keygrip)
    }
    fn inspect_ssh_agent(&mut self, expected: &OpenSshPublicKey) -> Result<SshAgentReadiness> {
        ssh_agent_backend::inspect_ssh_agent(expected)
    }
}
