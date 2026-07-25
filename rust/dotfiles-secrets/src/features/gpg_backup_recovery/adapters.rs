#[cfg(not(feature = "secrets-internal-test-stub"))]
mod gpg_cipher;
#[cfg(feature = "secrets-internal-test-stub")]
mod gpg_internal_stub;
#[cfg(not(feature = "secrets-internal-test-stub"))]
mod gpg_keyring;
#[cfg(not(feature = "secrets-internal-test-stub"))]
mod gpg_ssh_agent;
