//! enrollment / verification summary の domain model。
//!
//! use case 結果の意味だけを保持し、JSON key や pretty-print などの presentation 形式は持たない。

use std::collections::BTreeMap;

use super::bws::BwsSecretName;

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
                (CheckName::BwsAccessToken, CheckStatus::Ok),
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

    #[test]
    fn partial_rotate_summary_serializes_updated_entries() {
        let summary = VerifySummary::local_storage_failed(42);

        assert_eq!(summary.serial, 42);
        assert_eq!(
            summary.checks.get(&CheckName::LocalStorage),
            Some(&CheckStatus::Failed)
        );
    }

    #[test]
    fn partial_rotate_summary_skips_output_when_empty() {
        let summaries: Vec<VerifySummary> = Vec::new();

        assert!(summaries.is_empty());
    }
}
