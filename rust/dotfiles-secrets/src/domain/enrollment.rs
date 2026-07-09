//! YubiKey enrollment summary の domain model。
//!
//! primary/spare role と enrollment check の意味だけを保持し、report 形式や CLI 表現は持たない。

use std::collections::BTreeMap;

use super::verification::{CheckName, CheckStatus};

/// enrollment 対象の YubiKey role。
///
/// primary/spare の意味だけを表し、選択順序や presentation 文言は含めない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YubikeyRole {
    Primary,
    Spare,
}

/// enrollment use case の結果要約。
///
/// serial、role、各 check の意味結果を保持し、report 形式や JSON key は含めない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrollSummary {
    pub serial: u32,
    pub role: YubikeyRole,
    pub checks: BTreeMap<CheckName, CheckStatus>,
}

impl EnrollSummary {
    /// primary YubiKey enrollment の完了結果を構築する。
    ///
    /// setup と secret 書き込みが完了し、local storage check まで成功した summary を返す。
    pub fn primary_completed(serial: u32) -> Self {
        Self::completed(serial, YubikeyRole::Primary)
    }

    /// spare YubiKey enrollment の完了結果を構築する。
    ///
    /// primary 完了時と同じ check 意味を保ちつつ role だけを spare に固定する。
    pub fn spare_completed(serial: u32) -> Self {
        Self::completed(serial, YubikeyRole::Spare)
    }

    /// enrollment 完了直後の domain summary を構築する。
    ///
    /// setup と secret checks は成功、local storage は未検証として初期化する。
    /// 呼び出し側は local storage 検証後に `mark_local_storage_ok` で状態を更新する責務を負う。
    pub fn initial(serial: u32, role: YubikeyRole) -> Self {
        Self {
            serial,
            role,
            checks: [
                (CheckName::Setup, CheckStatus::Ok),
                (CheckName::BwEmail, CheckStatus::Ok),
                (CheckName::BwPassword, CheckStatus::Ok),
                (CheckName::BitwardenClientId, CheckStatus::Ok),
                (CheckName::BitwardenClientSecret, CheckStatus::Ok),
                (CheckName::LocalStorage, CheckStatus::Skipped),
            ]
            .into_iter()
            .collect(),
        }
    }

    /// local storage 検証が成功したことを summary へ反映する。
    ///
    /// この更新は `LocalStorage` check だけを書き換え、他の check 結果は保持する。
    pub fn mark_local_storage_ok(&mut self) {
        self.checks.insert(CheckName::LocalStorage, CheckStatus::Ok);
    }

    fn completed(serial: u32, role: YubikeyRole) -> Self {
        let mut summary = Self::initial(serial, role);
        summary.mark_local_storage_ok();
        summary
    }
}
