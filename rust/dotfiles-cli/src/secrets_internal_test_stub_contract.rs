//! Internal stub process wiring contract for CLI integration tests.
//!
//! This module is compiled only with `secrets-internal-test-stub`. It exposes
//! env var names shared by the integration-test process launcher and the
//! feature-gated adapter backend stubs. It does not expose backend datastore
//! schema, fixture expansion, or state helpers.

pub const YUBIKEY_STUB_SPEC_ENV: &str = "DOTFILES_SECRETS_YUBIKEY_STUB_SPEC_JSON";
pub const YUBIKEY_STUB_OUTPUT_ENV: &str = "DOTFILES_SECRETS_YUBIKEY_STUB_OUTPUT_PATH";
pub const BWS_STUB_SPEC_ENV: &str = "DOTFILES_SECRETS_BWS_STUB_SPEC_JSON";
pub const BWS_STUB_OUTPUT_ENV: &str = "DOTFILES_SECRETS_BWS_STUB_OUTPUT_PATH";
