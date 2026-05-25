//! `dotfiles secrets` の利用者入力 adapter 集約。
//!
//! 各責務は個別の adapter module に委譲する。

pub(crate) use super::enrollment_json::read_enrollment_json_bytes;
pub(crate) use super::prompt::{read_hidden_bytes, read_visible_line_bytes, read_yubikey_pin_raw};
pub(crate) use super::stdin::read_stdin_bytes;
pub(crate) use super::stdout::write_secret_to_stdout;
