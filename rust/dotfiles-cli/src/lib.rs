//! `dotfiles` CLI のライブラリ境界。
//!
//! 利用者向け binary はこの crate の公開 entrypoint を呼ぶ薄い層に限定する。
//! CLI 統合テストは通常の `dotfiles` binary ではなく、Cargo が
//! `secrets-internal-test-stub` feature 付きで事前に build した専用
//! `dotfiles-secrets-internal-test-stub` binary を起動する。この binary も本 crate の
//! `dispatch` を呼ぶため command dispatch は利用者向け binary と同一である。
//!
//! 専用 target は `required-features` で featureless な通常 artifact と分離される。feature により
//! `dotfiles-secrets` の YubiKey adapter は compile-time internal stub に置換されるため、統合テストの
//! child process が実機 YubiKey backend へ到達することはない。

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
/// `42` は低水準 `yubikey status` の観測済み予約 storage 不整合、`43` は低水準
/// `yubikey put` の完全未初期化を表す互換的な公開終了コードとして返す。provisioning script は
/// これらを state transition の根拠にせず、`provision-bws-token` 一回へ遷移全体を委譲する。
/// その他の失敗は fail-closed で通常の失敗コードにする。
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
