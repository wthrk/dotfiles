//! `dotfiles secrets` application 層の出力 summary 型。
//!
//! summary JSON の契約は use case 完了時の報告責務に属するため、この層で保持する。

use std::collections::BTreeMap;

use serde::Serialize;

use crate::secrets::domain::YubikeyRole;

/// summary に出す確認項目の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum CheckStatus {
    /// 確認に成功した状態。
    #[serde(rename = "ok")]
    Ok,
    /// 永続書き込み後の確認に失敗した状態。
    #[serde(rename = "failed")]
    Failed,
    /// 現在の実行範囲では省略した確認項目。
    #[serde(rename = "skipped")]
    Skipped,
}

/// summary JSON の `checks` key として使う閉じた確認項目名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CheckName {
    Setup,
    BwEmail,
    BwPassword,
    BwsAccessToken,
    LocalStorage,
    Bws,
    BwLogin,
}

/// enroll 系 command の成功 summary。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct EnrollSummary {
    pub(crate) serial: u32,
    pub(crate) role: YubikeyRole,
    pub(crate) checks: BTreeMap<CheckName, CheckStatus>,
}

/// verify 系 command の成功 summary。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct VerifySummary {
    pub(crate) serial: u32,
    pub(crate) checks: BTreeMap<CheckName, CheckStatus>,
}
