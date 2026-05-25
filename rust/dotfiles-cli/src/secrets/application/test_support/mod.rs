//! application 層 unit test ヘルパーとテストモジュール。
//!
//! fake 境界実装と storage service の契約確認はこのモジュールに集約する。
//! production コード（`application/`直下）と物理的に分離するために `application/test_support/` 配下に置く。

pub(super) mod fake_boundary;
mod storage_service_tests;
