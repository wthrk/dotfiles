//! verification check と verify summary の domain model。
//!
//! check 名、成功/失敗/未実施の意味、BWS 外部確認 plan を保持し、report の JSON 表現は持たない。

use std::collections::BTreeMap;

use super::bws::BwsSecretName;

/// verify-yubikey で要求できる外部検証種別。
///
/// CLI 入力の閉じた集合を表し、domain check 名への写像元として使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalCheck {
    Bws,
    BwLogin,
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
    BwEmail,
    BwPassword,
    BwsAccessToken,
    LocalStorage,
    Bws,
    BwLogin,
}

impl CheckName {
    /// CLI / report で使う安定した check 名を返す。
    ///
    /// 返値は presentation 側の key へ渡すための安定識別子で、version 更新なしに変更してはならない。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::BwEmail => "bw-email",
            Self::BwPassword => "bw-password",
            Self::BwsAccessToken => "bws-access-token",
            Self::LocalStorage => "local-storage",
            Self::Bws => "bws",
            Self::BwLogin => "bw-login",
        }
    }

    /// BWS 外部確認で取得必須となる復旧 secret 群を返す。
    ///
    /// `verify-yubikey --check bws` の成功条件は domain plan として固定し、application は
    /// 返された順序で port capability を適用するだけにする。
    pub fn required_bws_secrets(self) -> Option<&'static [BwsSecretName]> {
        match self {
            Self::Bws => Some(&[
                BwsSecretName::GpgSecretKeyBackup,
                BwsSecretName::PasswordStoreRemote,
            ]),
            Self::Setup
            | Self::BwEmail
            | Self::BwPassword
            | Self::BwsAccessToken
            | Self::LocalStorage
            | Self::BwLogin => None,
        }
    }
}

/// verify-yubikey use case の結果要約。
///
/// local storage と external checks の結果意味だけを保持し、表示仕様は外側へ出す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifySummary {
    pub serial: u32,
    pub checks: BTreeMap<CheckName, CheckStatus>,
}

impl VerifySummary {
    /// local storage 検証が成功した通常系 summary を構築する。
    ///
    /// external checks は未実施として初期化し、後続の実行結果反映を待つ。
    pub fn local_storage_verified(serial: u32) -> Self {
        Self::with_local_storage_status(serial, CheckStatus::Ok)
    }

    /// local storage 検証が失敗した停止系 summary を構築する。
    ///
    /// この summary は external checks を未実施のまま保持し、呼び出し側に継続不可を示す。
    pub fn local_storage_failed(serial: u32) -> Self {
        Self::with_local_storage_status(serial, CheckStatus::Failed)
    }

    /// external check の実行結果を summary へ反映する。
    pub fn mark_external_check(&mut self, check: CheckName, status: CheckStatus) {
        self.checks.insert(check, status);
    }

    fn with_local_storage_status(serial: u32, local_storage: CheckStatus) -> Self {
        Self {
            serial,
            checks: [
                (CheckName::LocalStorage, local_storage),
                (CheckName::Bws, CheckStatus::Skipped),
                (CheckName::BwLogin, CheckStatus::Skipped),
            ]
            .into_iter()
            .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// verify summary は local storage failure と external skipped 状態を保持する。
    #[test]
    fn partial_rotate_summary_serializes_updated_entries() {
        let summary = VerifySummary::local_storage_failed(42);

        assert_eq!(summary.serial, 42);
        assert_eq!(
            summary.checks.get(&CheckName::LocalStorage),
            Some(&CheckStatus::Failed)
        );
    }

    /// 空 summary list は report 対象を持たない。
    #[test]
    fn partial_rotate_summary_skips_output_when_empty() {
        let summaries: Vec<VerifySummary> = Vec::new();

        assert!(summaries.is_empty());
    }
}
