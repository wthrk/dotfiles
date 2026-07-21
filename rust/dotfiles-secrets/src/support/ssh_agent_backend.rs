//! gpg-agent SSH concrete backend operations.
use crate::{
    Result,
    domain::gpg_restore::{Keygrip, OpenSshPublicKey, SshAgentReadiness},
    support::{
        ssh_agent_protocol::{request_identities, sshcontrol_contains, sshcontrol_path},
        ssh_agent_socket::{gnupg_home, resolve_ssh_agent_socket},
    },
};
use anyhow::Context;
use std::{fs::OpenOptions, io::Write};
pub(crate) fn register_authentication_subkey(keygrip: &Keygrip) -> Result<()> {
    let path = sshcontrol_path(gnupg_home()?);
    if sshcontrol_contains(&path, keygrip)? {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .context("failed to create GnuPG home directory for sshcontrol")?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .context("failed to open gpg-agent sshcontrol")?;
    writeln!(file, "{}", keygrip.as_str())
        .context("failed to register keygrip in gpg-agent sshcontrol")
}
pub(crate) fn inspect_ssh_agent(
    expected_public_key: &OpenSshPublicKey,
) -> Result<SshAgentReadiness> {
    let socket = resolve_ssh_agent_socket()?;
    let socket_resolved = socket.is_some();
    let recovery_identity_present = match socket {
        Some(path) => request_identities(&path)
            .context("failed to inspect resolved SSH agent identities")?
            .iter()
            .any(|blob| expected_public_key.matches_agent_key_blob(blob)),
        None => false,
    };
    Ok(SshAgentReadiness {
        socket_resolved,
        recovery_identity_present,
    })
}
