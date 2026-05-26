//! `dotfiles` CLI のライブラリ境界。
//!
//! 利用者向け binary はこの crate の `cli::dispatch` を呼ぶ薄い entrypoint に限定する。
//! CLI 統合テストは production 経路と同じ application / port / domain を再利用しつつ、
//! 実機 YubiKey の代わりとなる test double を tests 層の専用 crate（`dotfiles-cli-secrets-test-stub`）
//! 側に持つ。本 crate は production 成果物だけを公開し、test double の定義を含めない。

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

/// tests 層の stub crate が production 経路を駆動するための公開 seam。
///
/// port 契約型・domain wire format・application entrypoint 関数を公開する。
/// adapter 具体型（`RealSecretsBoundary` 等）は tests 層から直接参照させない。
/// test double（実依存を肩代わりする型）は本 crate に置かない。
pub use secrets::domain;
pub use secrets::ports;
pub use secrets::run_with_args;
pub use secrets::{CheckName, CheckStatus, EnrollSummary, VerifySummary, YubikeyRole};
