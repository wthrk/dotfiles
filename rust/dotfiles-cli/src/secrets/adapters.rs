//! `dotfiles secrets` の外部 I/O adapter モジュール。
//!
//! 実プロセスの stdin/stdout/terminal 境界と実機 YubiKey device の discovery / open /
//! PIV 操作翻訳は `process_boundary` に集約する。test double（in-memory stub device）は
//! 本層に置かず、tests 層の専用 crate が所有する。
//!
//! 実プロセス境界の組み立て（`RealSecretsBoundary` の構築）は、このモジュールの呼び出し元
//! (`secrets` module root) が行う。adapter 層は port 実装と外部技術翻訳のみを担い、
//! 境界組み立て工場関数を公開しない。

pub(super) mod process_boundary;
