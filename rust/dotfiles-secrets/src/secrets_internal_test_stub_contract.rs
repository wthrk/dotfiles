//! Internal stub process wiring contract for CLI integration tests.
//!
//! This module is compiled only with `secrets-internal-test-stub`. It exposes
//! fixture/spec env var names and stdout observation framing shared by the
//! integration-test process launcher and the feature-gated adapter backend
//! stubs. It does not expose backend datastore schema, fixture expansion, or
//! state helpers.

pub const YUBIKEY_STUB_SPEC_ENV: &str = "DOTFILES_SECRETS_YUBIKEY_STUB_SPEC_JSON";
pub const BWS_STUB_SPEC_ENV: &str = "DOTFILES_SECRETS_BWS_STUB_SPEC_JSON";
pub const GPG_STUB_SPEC_ENV: &str = "DOTFILES_SECRETS_GPG_STUB_SPEC_JSON";
pub const GIT_STUB_SPEC_ENV: &str = "DOTFILES_SECRETS_GIT_STUB_SPEC_JSON";
pub const STUB_OBSERVATION_PREFIX: &str = "__DOTFILES_SECRETS_STUB_OBSERVATION__";

#[cfg(feature = "secrets-internal-test-stub")]
mod secret_input_observation {
    use std::sync::atomic::{AtomicU32, Ordering};

    static ATTEMPTS: AtomicU32 = AtomicU32::new(0);

    pub(super) fn record() {
        ATTEMPTS.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn count() -> u32 {
        ATTEMPTS.load(Ordering::Relaxed)
    }
}

#[cfg(feature = "secrets-internal-test-stub")]
pub fn record_secret_input_attempt() {
    secret_input_observation::record();
}

#[cfg(feature = "secrets-internal-test-stub")]
pub fn secret_input_attempt_count() -> u32 {
    secret_input_observation::count()
}
