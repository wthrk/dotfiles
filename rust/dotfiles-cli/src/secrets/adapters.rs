//! `dotfiles secrets` の外部 I/O adapter。
//!
//! YubiKey device の実装差を `SecretDevice` port に閉じ、application へ同じ device contract を渡す。
//! 実プロセスの stdin/stdout/terminal 境界と実機 device の discovery / open は `process_boundary`
//! に集約する。device の開き方は `ports::SecretDeviceFactory` の seam として切り出し、CLI 統合
//! テストの代替 device 実装（test double）は本層に置かず tests 層の専用 crate が所有する。

pub(super) mod process_boundary;
mod yubikey;

use crate::Result;

/// 実機 YubiKey を開く実プロセス用の `SecretsBoundary` 実装を構築して返す。
///
/// device の discovery / open は実機 boundary に閉じ、呼び出し元は境界型だけを受け取る。
pub(crate) fn build_real_boundary() -> Result<impl crate::secrets::ports::SecretsBoundary> {
    process_boundary::RealSecretsBoundary::new(false)
}
