//! `dotfiles secrets` の利用者入力 adapter 集約。
//!
//! 各責務は個別の adapter module に委譲する。

pub(crate) use super::enrollment_json::{
    read_protected_enrollment_secret_set, EnrollmentSecretSet, MAX_BOOTSTRAP_JSON_LEN,
};
pub(crate) use super::prompt::{read_hidden_secret, read_visible_secret_line, read_yubikey_pin};
pub(crate) use super::stdin::{read_protected_stdin_secret, MAX_SINGLE_STDIN_SECRET_LEN};
pub(crate) use super::stdout::{reject_secret_stdout_terminal, write_secret_to_stdout};
