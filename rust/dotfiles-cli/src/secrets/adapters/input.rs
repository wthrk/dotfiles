//! `dotfiles secrets` の利用者入力 adapter 集約。
//!
//! 各責務は個別の adapter module に委譲する。

pub(crate) use super::enrollment_json::read_protected_enrollment_secret_set;
pub(crate) use super::prompt::{read_hidden_secret, read_visible_secret_line, read_yubikey_pin};
pub(crate) use super::stdin::read_protected_stdin_secret;
pub(crate) use super::stdout::write_secret_to_stdout;
