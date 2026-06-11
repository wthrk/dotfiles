//! `dotfiles` CLI のライブラリ境界。
//!
//! 利用者向け binary はこの crate の公開 entrypoint を呼ぶ薄い層に限定する。
//! CLI 統合テストは `dotfiles` binary を直接実行して production 経路を検証する。

pub(crate) mod cli;
mod environment;
mod init;
mod local_flake;
mod process;
mod switch;
mod update;
mod update_history;

/// CLI の各 command が共有する結果型。
pub type Result<T> = dotfiles_core::Result<T>;

/// crate 公開の CLI 実行 entrypoint。
pub fn dispatch() -> Result<()> {
    cli::dispatch()
}
