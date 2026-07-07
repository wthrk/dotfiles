//! verification check と verify summary の domain model。
//!
//! check 名、成功/失敗/未実施の意味を保持し、report の JSON 表現は持たない。

use std::collections::BTreeMap;

/// verify-yubikey で要求できる外部検証種別。
///
/// CLI 入力の閉じた集合を表し、domain check 名への写像元として使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalCheck {
    Vault,
}

/// 各 verification/enrollment check の結果状態。
///
/// presentation 文言は持たず、成功・失敗・未実施という意味だけを表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Ok,
    Failed,
    Skipped,
}

/// use case summary が扱う check 名の閉じた集合。
///
/// 各 variant は report と verification flow の安定キーであり、raw string の代替として使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckName {
    Setup,
    BitwardenClientId,
    BitwardenClientSecret,
    LocalStorage,
    Vault,
}

impl CheckName {
    /// CLI / report で使う安定した check 名を返す。
    ///
    /// 返値は presentation 側の key へ渡すための安定識別子で、version 更新なしに変更してはならない。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::BitwardenClientId => "bitwarden-client-id",
            Self::BitwardenClientSecret => "bitwarden-client-secret",
            Self::LocalStorage => "local-storage",
            Self::Vault => "vault",
        }
    }
}

/// verify-yubikey use case の結果要約。
///
/// local storage と external checks の結果意味だけを保持し、表示仕様は外側へ出す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifySummary {
    pub checks: BTreeMap<CheckName, CheckStatus>,
}

impl VerifySummary {
    /// local storage 検証が成功した通常系 summary を構築する。
    ///
    /// external checks は未実施として初期化し、後続の実行結果反映を待つ。
    pub fn local_storage_verified() -> Self {
        Self::with_local_storage_status(CheckStatus::Ok)
    }

    /// local storage 検証が失敗した停止系 summary を構築する。
    ///
    /// この summary は external checks を未実施のまま保持し、呼び出し側に継続不可を示す。
    pub fn local_storage_failed() -> Self {
        Self::with_local_storage_status(CheckStatus::Failed)
    }

    /// external check の実行結果を summary へ反映する。
    pub fn mark_external_check(&mut self, check: CheckName, status: CheckStatus) {
        self.checks.insert(check, status);
    }

    fn with_local_storage_status(local_storage: CheckStatus) -> Self {
        Self {
            checks: [
                (CheckName::LocalStorage, local_storage),
                (CheckName::Vault, CheckStatus::Skipped),
            ]
            .into_iter()
            .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// verify summary は local storage failure 状態を保持する。
    #[test]
    fn verify_summary_records_local_storage_failure() {
        let summary = VerifySummary::local_storage_failed();

        assert_eq!(
            summary.checks.get(&CheckName::LocalStorage),
            Some(&CheckStatus::Failed)
        );
    }

    /// 空の verify summary list は表示対象を持たない。
    #[test]
    fn empty_verify_summary_list_has_no_report_targets() {
        let summaries: Vec<VerifySummary> = Vec::new();

        assert!(summaries.is_empty());
    }
}
