//! `dotfiles secrets` の adapter 層公開面。
//!
//! entrypoint が起動する runtime 境界実装 module を内包し、port 実装以外は外部公開しない。

mod piv_io;
mod yubikey;

pub(super) use piv_io::SelectedSecretsBoundary;

/// `dotfiles secrets ...` 既存ルートで使う production 境界を返す。
///
/// CLI 引数や command 種別では分岐せず、adapter 側の concrete 実装だけを差し替え可能な seam。
pub(super) fn select_secrets_boundary() -> SelectedSecretsBoundary {
    piv_io::build_selected_secrets_boundary()
}
