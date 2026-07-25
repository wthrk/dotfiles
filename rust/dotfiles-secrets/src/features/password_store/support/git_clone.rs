//! private password-store の git2/libssh2 SSH-agent clone を閉じる technical backend。

#[cfg(all(not(test), not(feature = "secrets-internal-test-stub")))]
use anyhow::Context;
#[cfg(not(feature = "secrets-internal-test-stub"))]
use git2::Error as GitError;
#[cfg(all(not(test), not(feature = "secrets-internal-test-stub")))]
use git2::{
    CertificateCheckStatus, Cred, CredentialType, FetchOptions, RemoteCallbacks, build::RepoBuilder,
};

#[cfg(all(not(test), not(feature = "secrets-internal-test-stub")))]
use crate::features::password_store::domain::pass_restore::PASSWORD_STORE_DIR_NAME;
#[cfg(all(not(test), not(feature = "secrets-internal-test-stub")))]
use crate::features::password_store::support::filesystem::home_child;
#[cfg(all(not(test), not(feature = "secrets-internal-test-stub")))]
use crate::features::password_store::support::github_ssh_host_key;
use crate::{Result, features::password_store::domain::pass_restore::PasswordStoreRemote};

pub(crate) fn clone_password_store(remote: &PasswordStoreRemote) -> Result<()> {
    let _ = remote;
    anyhow::bail!("password-store clone requires the gpg-agent socket capability")
}

#[cfg(all(not(test), not(feature = "secrets-internal-test-stub")))]
pub(crate) fn clone_password_store_with_socket(
    remote: &PasswordStoreRemote,
    gpg_agent_socket: &mut dyn crate::features::gpg_backup_recovery::ports::public::GpgAgentSocketPort,
) -> Result<()> {
    let socket = gpg_agent_socket
        .resolve_strict_socket()?
        .context("could not resolve the gpg-agent SSH agent socket for password-store clone")?;
    let previous_sock = std::env::var_os("SSH_AUTH_SOCK");
    // SAFETY: this single-threaded command temporarily selects the non-secret, strict gpg-agent socket.
    unsafe { std::env::set_var("SSH_AUTH_SOCK", &socket) };
    let _restore_sock = scopeguard::guard(previous_sock, |previous| {
        // SAFETY: restores exactly the prior non-secret process environment value.
        unsafe {
            match previous {
                Some(value) => std::env::set_var("SSH_AUTH_SOCK", value),
                None => std::env::remove_var("SSH_AUTH_SOCK"),
            }
        }
    });

    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, allowed_types| {
        if allowed_types.contains(CredentialType::SSH_KEY) {
            Cred::ssh_key_from_agent(username_from_url.unwrap_or("git"))
        } else {
            Err(git2::Error::from_str(
                "password-store clone requires SSH agent authentication",
            ))
        }
    });
    callbacks.certificate_check(|cert, hostname| {
        match github_ssh_host_key::verify(cert, hostname) {
            Ok(()) => Ok(CertificateCheckStatus::CertificateOk),
            Err(message) => Err(git2::Error::from_str(&message)),
        }
    });

    let destination = home_child(PASSWORD_STORE_DIR_NAME)?;
    match std::fs::create_dir(&destination) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            anyhow::bail!("refusing to clone over an existing ~/.password-store");
        }
        Err(error) => {
            return Err(anyhow::Error::new(error)
                .context("failed to claim ~/.password-store before cloning"));
        }
    }
    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);
    if let Err(error) = RepoBuilder::new()
        .fetch_options(fetch_options)
        .clone(remote.as_str(), &destination)
    {
        let cleanup = std::fs::remove_dir_all(&destination);
        return Err(safe_clone_failure(error, cleanup.err()));
    }
    Ok(())
}

/// git2/server supplied error の文字列を利用者向け error chain へ保持しない。
///
/// remote URL や server diagnostic は private repository の所在を含み得るため、clone failure の
/// presentation は固定の安全文脈に限定する。
#[cfg(not(feature = "secrets-internal-test-stub"))]
fn safe_clone_failure(error: GitError, cleanup: Option<std::io::Error>) -> anyhow::Error {
    let source = anyhow::Error::new(error);
    let source = match cleanup {
        Some(cleanup) => {
            source.context(format!("clone destination cleanup also failed: {cleanup}"))
        }
        None => source,
    };
    anyhow::Error::new(PrivateCloneFailure { source })
}

/// 外部の clone / cleanup 原因を source chain に保持しつつ、利用者へは private remote を含まない固定文言だけを出す。
#[cfg(not(feature = "secrets-internal-test-stub"))]
#[derive(Debug)]
struct PrivateCloneFailure {
    source: anyhow::Error,
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
impl std::fmt::Display for PrivateCloneFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "failed to clone private password-store over SSH")
    }
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
impl std::error::Error for PrivateCloneFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::safe_clone_failure;

    #[test]
    fn clone_error_with_remote_is_not_rendered_for_the_user() {
        let remote = "git@github.com:private-owner/private-password-store.git";
        let error = safe_clone_failure(
            git2::Error::from_str(&format!("server rejected {remote}")),
            None,
        );

        assert_eq!(
            error.to_string(),
            "failed to clone private password-store over SSH"
        );
        assert!(!error.to_string().contains(remote));
    }
}
