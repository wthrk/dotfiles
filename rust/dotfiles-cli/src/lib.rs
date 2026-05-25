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

/// CLI 統合テストの専用 stub binary が production 経路を駆動するための公開境界。
///
/// この module は production binary からは参照されず、tests 層の stub crate だけが依存する。
/// 公開するのは port 契約・domain wire format・実プロセス境界の組立 seam に限り、test double
/// （実依存を肩代わりする型）は本 crate に置かない。
pub mod testing {
    pub use crate::secrets::testing::*;
}
