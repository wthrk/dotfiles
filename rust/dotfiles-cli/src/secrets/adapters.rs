//! `dotfiles secrets` の外部 I/O adapter。
//!
//! YubiKey device の実装差を `SecretDevice` port に閉じ、application へ同じ device contract を渡す。

mod real_boundary;
#[cfg(feature = "secrets-test-stub")]
mod test_stub;
mod yubikey;

use crate::Result;

/// 実プロセス用の `SecretsBoundary` 実装を構築して返す。
///
/// backend の選択ロジックは adapter 層に閉じ、呼び出し元は境界型だけを受け取る。
pub(super) fn build_real_boundary(test_stub: bool) -> Result<impl crate::secrets::ports::SecretsBoundary> {
    real_boundary::RealSecretsBoundary::new(test_stub)
}
