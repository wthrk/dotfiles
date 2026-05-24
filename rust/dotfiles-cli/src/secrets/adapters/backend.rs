//! YubiKey device backend 選択 adapter。
//!
//! 実機と test-stub の選択状態を保持し、application へ同じ device contract を渡す。

#[cfg(feature = "secrets-test-stub")]
use super::test_stub;
use crate::Result;

#[cfg(feature = "secrets-test-stub")]
/// CLI 実行で使う YubiKey device adapter の選択状態。
///
/// application はこの値を保持するだけで、実機か stub かに応じた別 use case を持たない。
pub(crate) enum DeviceBackend {
    /// 実機 YubiKey adapter を使う通常実行。
    Real,
    /// CLI 統合テスト用の in-memory device adapter を使う実行。
    TestStub(test_stub::TestDeviceFactory),
}

#[cfg(not(feature = "secrets-test-stub"))]
#[derive(Clone, Copy)]
/// CLI 実行で使う YubiKey device adapter の選択状態。
///
/// 通常 build では実機 adapter だけを持ち、stub 用の実行経路を含めない。
pub(crate) enum DeviceBackend {
    /// 実機 YubiKey adapter を使う通常実行。
    Real,
}

impl DeviceBackend {
    #[cfg(feature = "secrets-test-stub")]
    /// CLI option から device adapter の選択状態を構築する。
    ///
    /// `secrets-test-stub` feature 有効時だけ hidden test flag を解釈し、stub の初期状態は
    /// integration test contract の環境変数から読む。
    pub(crate) fn from_test_flag(enabled: bool) -> Result<Self> {
        if enabled {
            return Ok(Self::TestStub(test_stub::TestDeviceFactory::from_env()?));
        }
        Ok(Self::Real)
    }

    #[cfg(not(feature = "secrets-test-stub"))]
    /// 通常 build で実機 adapter の選択状態を構築する。
    ///
    /// stub 用 flag は clap 定義に存在しないため、この build では常に実機 adapter を選ぶ。
    pub(crate) fn from_test_flag(_enabled: bool) -> Result<Self> {
        Ok(Self::Real)
    }
}
