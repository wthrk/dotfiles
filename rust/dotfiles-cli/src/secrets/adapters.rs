//! `dotfiles secrets` の外部 I/O adapter。
//!
//! 実プロセスの stdin/stdout/terminal 境界と実機 YubiKey device の discovery / open は
//! `process_boundary` に集約する。test double（in-memory stub device）は本層に置かず、
//! tests 層の専用 crate が所有する。

pub(super) mod process_boundary;
mod yubikey;

/// 実機 YubiKey を使う実プロセス用の `SecretsBoundary` 実装を構築して返す。
///
/// 呼び出し元は境界型だけを受け取り、device の discovery / open の詳細を知らない。
pub(crate) fn build_real_boundary() -> impl crate::secrets::ports::SecretsBoundary {
    process_boundary::RealSecretsBoundary::new()
}
