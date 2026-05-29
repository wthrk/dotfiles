#![cfg(feature = "secrets-internal-test-stub")]
//! `secrets-internal-test-stub` feature の integration test target。
//!
//! production binary を feature で test double へ差し替える構成は採用しない。
//! secret-recovery の実行経路確認は `src/` 側の unit/application test で port 契約を通して行う。
