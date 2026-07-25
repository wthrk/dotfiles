pub(crate) mod backend;
#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
pub(crate) mod filesystem;
#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
pub(crate) mod git_clone;
#[cfg(all(
    feature = "gpg-backend",
    not(feature = "secrets-internal-test-stub"),
    not(test)
))]
pub(crate) mod github_ssh_host_key;
#[cfg(feature = "secrets-internal-test-stub")]
pub(crate) mod internal_stub_git;
#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
pub(crate) mod password_store;
