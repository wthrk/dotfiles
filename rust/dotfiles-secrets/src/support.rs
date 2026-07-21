//! 秘密値処理で再利用する utility と protection backend 境界。
//!
//! process / memory 保護、暗号 primitive 補助、secret を扱う外部処理の保護境界をここに置く。

pub(crate) mod adapter_backend;
#[cfg(not(feature = "secrets-internal-test-stub"))]
pub(crate) mod aead;
#[cfg(not(feature = "secrets-internal-test-stub"))]
pub(crate) mod bws_sdk;
pub(crate) mod clock;
#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
pub(crate) mod filesystem;
#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
pub(crate) mod git_clone;
#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
pub(crate) mod github_ssh_host_key;
#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
pub(crate) mod gpg_keyring;
#[cfg(feature = "secrets-internal-test-stub")]
pub(crate) mod internal_stub_bws;
#[cfg(feature = "secrets-internal-test-stub")]
pub(crate) mod internal_stub_git;
#[cfg(feature = "secrets-internal-test-stub")]
pub(crate) mod internal_stub_gpg;
#[cfg(feature = "secrets-internal-test-stub")]
pub(crate) mod internal_stub_yubikey;
#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
pub(crate) mod password_store;
pub(crate) mod piv_storage;
pub(crate) mod process_io;
pub(crate) mod protection;
pub(crate) mod report;
#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
pub(crate) mod ssh_agent_protocol;
#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
pub(crate) mod ssh_agent_socket;
pub(crate) mod yubikey_backend;
pub(crate) mod yubikey_device_serial;
pub(crate) mod yubikey_storage;
