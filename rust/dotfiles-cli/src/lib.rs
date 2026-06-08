//! `dotfiles` CLI のライブラリ境界。
//!
//! 利用者向け binary はこの crate の公開 entrypoint を呼ぶ薄い層に限定する。
//! CLI 統合テストは `dotfiles` binary を直接実行して production 経路を検証する。

mod ci;
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
///
/// 終了コードを直接返す（`update` の lock 競合 skip 専用 code を含む。詳細は [`cli::dispatch`]）。binary 側は
/// この `ExitCode` をそのままプロセス終了コードへ渡す。
pub async fn dispatch() -> std::process::ExitCode {
    cli::dispatch().await
}
