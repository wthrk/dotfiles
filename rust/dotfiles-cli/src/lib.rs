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

/// command error を利用者向け終了コードへ変換する。
///
/// `yubikey status` が観測済みの予約 storage 不整合を報告した場合だけ、provisioning script
/// が clear を許可できる専用コードを返す。その他の失敗は fail-closed で通常の失敗コードにする。
pub fn exit_code_for_error(error: &anyhow::Error) -> u8 {
    if dotfiles_secrets::is_secret_storage_status_invalid(error) {
        dotfiles_secrets::SECRET_STORAGE_STATUS_INVALID_EXIT_CODE
    } else if dotfiles_secrets::is_secret_storage_uninitialized(error) {
        dotfiles_secrets::SECRET_STORAGE_UNINITIALIZED_EXIT_CODE
    } else {
        1
    }
}

/// crate 公開の CLI 実行 entrypoint。
pub fn dispatch() -> Result<()> {
    cli::dispatch()
}
