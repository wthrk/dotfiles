//! application 層が利用者向け JSON 出力として所有する summary 型。

use std::collections::BTreeMap;

use serde::Serialize;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum CheckStatus {
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "skipped")]
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum YubikeyRole {
    Primary,
    Spare,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct EnrollSummary {
    pub(crate) serial: u32,
    pub(crate) role: YubikeyRole,
    pub(crate) checks: BTreeMap<CheckName, CheckStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct VerifySummary {
    pub(crate) serial: u32,
    pub(crate) checks: BTreeMap<CheckName, CheckStatus>,
}
