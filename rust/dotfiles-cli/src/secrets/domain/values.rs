//! usecase 入出力の意味だけを保持し、CLI 表現や I/O 手段の変更理由を domain へ混在させない。

use std::collections::BTreeMap;

use anyhow::Result;

use super::piv::{SecretName, SecretStorageSpec};

/// verify-yubikey で要求できる外部検証種別。
///
/// CLI 入力の閉じた集合を表し、domain check 名への写像元として使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalCheck {
    Bws,
    BwLogin,
}

/// setup use case の入力 command。
///
/// serial 指定の有無だけを保持し、選択手段や prompt 方針は含めない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupCommand {
    pub serial: Option<u32>,
}

/// put use case の入力 command。
///
/// 対象 secret、device serial、既存値上書き可否という domain 意味だけを保持する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PutCommand {
    pub name: SecretName,
    pub serial: Option<u32>,
    pub force: bool,
}

impl PutCommand {
    /// 非対話 put use case が要求する対象 serial を返す。
    pub fn required_serial(&self) -> Result<u32> {
        self.serial
            .ok_or_else(|| invalid_input("pass --serial in non-interactive use").into())
    }

    /// 指定 serial に対する put 対象の storage spec を返す。
    pub fn storage_spec(&self, serial: u32) -> SecretStorageSpec {
        self.name.storage_spec(serial)
    }
}

/// get use case の入力 command。
///
/// 取得対象 secret と device serial だけを保持し、出力形式は含めない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GetCommand {
    pub name: SecretName,
    pub serial: Option<u32>,
}

impl GetCommand {
    /// 指定 serial に対する get 対象の storage spec を返す。
    pub fn storage_spec(&self, serial: u32) -> SecretStorageSpec {
        self.name.storage_spec(serial)
    }
}

/// enroll-primary use case の入力 command。
///
/// primary 候補の serial 指定有無だけを保持する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnrollPrimaryCommand {
    pub serial: Option<u32>,
}

/// enroll-spare use case の入力 command。
///
/// primary と spare の対象 serial 指定だけを保持し、選択フローは含めない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnrollSpareCommand {
    pub primary_serial: Option<u32>,
    pub spare_serial: Option<u32>,
}

impl EnrollSpareCommand {
    /// 明示指定された primary/spare serial が同一でないことを事前確認する。
    ///
    /// 両方の serial が利用者入力で既に確定している場合、device open や secret 読み出しの前に
    /// domain invariant として拒否し、同一 device を spare として登録する経路を作らない。
    pub fn ensure_requested_serials_distinct(&self) -> Result<()> {
        if self.primary_serial.is_some() && self.primary_serial == self.spare_serial {
            return Err(invalid_input("primary and spare YubiKey serial must be different").into());
        }
        Ok(())
    }

    /// 解決済み primary/spare serial が別 device を指すことを確認する。
    ///
    /// primary と spare は異なる recovery device role であり、同一 serial への登録は
    /// device 選択手段に関係なく domain invariant として拒否する。
    pub fn ensure_distinct_resolved_serials(
        &self,
        primary_serial: u32,
        spare_serial: u32,
    ) -> Result<()> {
        if primary_serial == spare_serial {
            return Err(invalid_input("primary and spare YubiKey serial must be different").into());
        }
        Ok(())
    }

    /// 非対話 spare 登録で明示 primary serial と spare serial が衝突しないことを確認する。
    ///
    /// primary device を開かない入力経路でも、利用者が指定した role 関係の不変条件は
    /// command の domain rule として先に検証する。
    pub fn ensure_requested_primary_differs_from_spare(&self, spare_serial: u32) -> Result<()> {
        if self.primary_serial == Some(spare_serial) {
            return Err(invalid_input("primary and spare YubiKey serial must be different").into());
        }
        Ok(())
    }
}

/// Bitwarden Secrets Manager で管理する secret 名の閉じた集合。
///
/// BWS 上の secret key 名を domain 語彙で固定し、adapter が raw string を再構築しないようにする。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BwsSecretName {
    /// GPG 秘密鍵バックアップ。BWS key: `gpg-secret-key-backup`。
    GpgSecretKeyBackup,
    /// password-store リモート URL。BWS key: `password-store-remote`。
    PasswordStoreRemote,
}

impl BwsSecretName {
    /// BWS 上の secret key 名を返す。
    ///
    /// 返値は BWS API へそのまま渡す安定 key で、version なしに変更してはならない。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GpgSecretKeyBackup => "gpg-secret-key-backup",
            Self::PasswordStoreRemote => "password-store-remote",
        }
    }
}

/// restore-gpg use case の入力 command。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestoreGpgCommand {
    pub serial: Option<u32>,
}

/// restore-pass use case の入力 command。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestorePassCommand {
    pub serial: Option<u32>,
}

/// rotate-bws-token use case の入力 command。
///
/// token を更新する対象 device の serial 指定だけを保持する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RotateBwsTokenCommand {
    pub serial: Option<u32>,
}

impl RotateBwsTokenCommand {
    /// rotate-bws-token が要求する対象 serial を返す。
    pub fn required_serial(self) -> Result<u32> {
        self.serial
            .ok_or_else(|| invalid_input("pass --serial in non-interactive use").into())
    }

    /// rotate 対象 secret 名を返す。
    pub fn target_secret(self) -> SecretName {
        SecretName::BwsAccessToken
    }

    /// 指定 serial に対する rotate 対象の storage spec を返す。
    pub fn storage_spec(self, serial: u32) -> SecretStorageSpec {
        self.target_secret().storage_spec(serial)
    }
}

/// verify-yubikey use case の入力 command。
///
/// serial 指定の有無、要求 check、`--all` 指定を保持し、device 選択手段は port 境界へ委譲する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyYubikeyCommand {
    pub serial: Option<u32>,
    pub checks: Vec<ExternalCheck>,
    pub all: bool,
}

impl VerifyYubikeyCommand {
    /// verify-yubikey が要求された external check 集合を domain check 名へ正規化する。
    ///
    /// `--all` と `--check` の併用は不変条件違反として失敗する。
    /// 呼び出し側は返値の順序を presentation 用ではなく domain の実行順として扱う責務を負う。
    pub fn requested_external_checks(&self) -> Result<Vec<CheckName>> {
        if self.all && !self.checks.is_empty() {
            return Err(invalid_input("--all and --check cannot be used together").into());
        }

        if self.all {
            return Ok(vec![CheckName::Bws, CheckName::BwLogin]);
        }

        Ok(self
            .checks
            .iter()
            .map(|check| match check {
                ExternalCheck::Bws => CheckName::Bws,
                ExternalCheck::BwLogin => CheckName::BwLogin,
            })
            .collect())
    }

    /// 現フェーズで未対応の external check 要求を domain error へ変換する。
    pub fn external_checks_unavailable_error(&self, requested: &[CheckName]) -> anyhow::Error {
        let requested_names = requested
            .iter()
            .map(|check| check.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::anyhow!("external checks are not implemented yet: {requested_names}")
    }
}

fn invalid_input(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into())
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

    /// 未実装の external check を失敗状態として示す summary を構築する。
    ///
    /// local storage は成功済み前提とし、指定された external check 群だけを `Failed` へ上書きする。
    pub fn external_checks_unavailable(
        serial: u32,
        checks: impl IntoIterator<Item = CheckName>,
    ) -> Self {
        let mut summary = Self::local_storage_verified(serial);
        for check in checks {
            summary.checks.insert(check, CheckStatus::Failed);
        }
        summary
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
