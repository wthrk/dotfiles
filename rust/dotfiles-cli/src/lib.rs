//! `dotfiles` CLI のライブラリ境界。
//!
//! 利用者向け binary はこの crate の `cli::dispatch` を呼ぶ薄い entrypoint に限定する。
//! CLI 統合テストは `dotfiles` binary を直接実行して production 経路を検証する。

pub(crate) mod cli;
mod environment;
mod init;
mod local_flake;
mod process;
mod secrets;
mod switch;
mod update;

/// CLI と secret-recovery の各層で共有する結果型。
pub type Result<T> = dotfiles_core::Result<T>;

pub use cli::dispatch;

/// `dotfiles secrets` の層境界で共有する公開 seam。
///
/// port 契約型・domain wire format を公開し、CLI 実行経路を `dotfiles` binary 側に固定する。
pub use secrets::domain;
pub use secrets::ports;
pub use secrets::{CheckName, CheckStatus, EnrollSummary, VerifySummary, YubikeyRole};
