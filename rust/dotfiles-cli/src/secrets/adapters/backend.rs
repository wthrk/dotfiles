//! YubiKey device backend 選択 adapter。
//!
//! 実機 adapter の選択状態を保持し、application へ同じ device contract を渡す。

use crate::Result;

#[derive(Clone, Copy)]
/// CLI 実行で使う YubiKey device adapter の選択状態。
///
/// 通常 build では実機 adapter だけを持ち、stub 用の実行経路を含めない。
pub(crate) enum DeviceBackend {
    /// 実機 YubiKey adapter を使う通常実行。
    Real,
}

impl DeviceBackend {
    /// 通常 build で実機 adapter の選択状態を構築する。
    pub(crate) fn from_test_flag(_enabled: bool) -> Result<Self> {
        Ok(Self::Real)
    }
}
