//! `dotfiles secrets` の外部 I/O adapter。
//!
//! YubiKey device の実装差を `SecretDevice` port に閉じ、application へ同じ device contract を渡す。

mod backend;
mod device_prompt;
mod enrollment_json;
mod input;
mod prompt;
mod real_boundary;
mod stdin;
mod stdout;
mod terminal;
#[cfg(feature = "secrets-test-stub")]
mod test_stub;
mod yubikey;

use crate::Result;

use backend::DeviceBackend;

/// 実プロセス用の `SecretsBoundary` 実装を構築して返す。
///
/// backend の選択ロジックは adapter 層に閉じ、呼び出し元は境界型だけを受け取る。
pub(super) fn build_real_boundary(test_stub: bool) -> Result<impl crate::secrets::ports::SecretsBoundary> {
    let backend = DeviceBackend::from_test_flag(test_stub)?;
    Ok(real_boundary::RealSecretsBoundary::new(backend))
}
