//! `dotfiles secrets` use case の入力 command を表す domain model。
//!
//! CLI option の parse 方式や prompt 手段は含めず、application が適用する対象、serial、
//! 上書き可否、外部 check 指定だけを保持する。

use anyhow::Result;

use super::{
    gpg_backup::PrimaryFingerprint,
    piv::{SecretName, SecretStorageSpec},
    verification::{CheckName, ExternalCheck},
};

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
/// serial 指定の有無、要求 check、`--all` 指定、bw-login 外部確認の `--email` override の有無を保持し、
/// device 選択手段は port 境界へ委譲する。`email_override` が `Some` のときだけ bw-login 外部確認で
/// YubiKey の `bw-email` を使わず override を使う（yubikey-secret-storage-design.md L286）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyYubikeyCommand {
    pub serial: Option<u32>,
    pub checks: Vec<ExternalCheck>,
    pub all: bool,
    pub email_override: Option<String>,
}

impl VerifyYubikeyCommand {
    /// verify-yubikey が要求された external check 集合を domain check 名へ正規化する。
    ///
    /// `--all` と `--check` の併用は不変条件違反として失敗する。
    /// 同じ check を複数回指定（例: `--check bw-login --check bw-login`）しても各 external check は
    /// 高々 1 回だけ実行するよう、出現順を保ったまま重複排除する。これにより bw-login 確認が二重実行され、
    /// 1 回目の login/unlock で `bw` CLI がログイン済みになった状態で 2 回目の `bw login` が
    /// 「already logged in」で失敗し、有効な credential / OTP でも検証全体を成功させられなくなることを防ぐ。
    /// 呼び出し側は返値の順序を presentation 用ではなく domain の実行順として扱う責務を負う。
    pub fn requested_external_checks(&self) -> Result<Vec<CheckName>> {
        if self.all && !self.checks.is_empty() {
            return Err(invalid_input("--all and --check cannot be used together").into());
        }

        if self.all {
            return Ok(vec![CheckName::Bws, CheckName::BwLogin]);
        }

        // 出現順を保ったまま重複を取り除き、同じ external check を 2 回以上実行しない。
        let mut checks = Vec::new();
        for check in &self.checks {
            let name = match check {
                ExternalCheck::Bws => CheckName::Bws,
                ExternalCheck::BwLogin => CheckName::BwLogin,
            };
            if !checks.contains(&name) {
                checks.push(name);
            }
        }
        Ok(checks)
    }
}

/// restore-gpg use case の入力 command。
///
/// `bws-access-token` を読み出す対象 YubiKey の serial 指定有無だけを保持し、device 選択手段や
/// envelope 取得手段は port 境界へ委譲する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestoreGpgCommand {
    pub serial: Option<u32>,
}

/// restore-pass use case の入力 command。
///
/// `bws-access-token` を読み出す対象 YubiKey の serial 指定有無だけを保持し、device 選択手段や
/// remote URL 取得手段、clone 手段は port 境界へ委譲する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestorePassCommand {
    pub serial: Option<u32>,
}

/// bw-login use case の入力 command。
///
/// `bw-email` / `bw-password` を読み出す対象 YubiKey の serial 指定有無と、`--email` override の有無だけを
/// 保持する。device 選択手段、YubiKey secret 取得手順、OTP 入力手段、`bw` CLI 実行詳細は port 境界へ委譲する。
/// `email_override` が `Some` のときだけ YubiKey の `bw-email` を使わず override を使う（spec L178）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwLoginCommand {
    pub serial: Option<u32>,
    pub email_override: Option<String>,
}

/// export-ssh-public-key use case の入力 command。
///
/// 出力対象の primary fingerprint だけを保持する。GitHub 登録用の OpenSSH 公開鍵出力以外の手段は持たない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSshPublicKeyCommand {
    pub primary_fingerprint: PrimaryFingerprint,
}

/// gpg-secret-key-backup の primary 登録 use case の入力 command。
///
/// export 対象の primary fingerprint と、recipient を作る対象 YubiKey の serial 指定有無を保持する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterGpgBackupCommand {
    pub primary_fingerprint: PrimaryFingerprint,
    pub serial: Option<u32>,
}

/// gpg-secret-key-backup への spare recipient 追加 use case の入力 command。
///
/// envelope を復号して同一 DEK を得るために使う「既存 recipient 機（unwrap 機）」の serial 指定有無、
/// 追加対象 spare YubiKey の serial 指定有無、非対話実行での明示上書き許可を保持する。対話実行では
/// project/secret 名と primary fingerprint を表示して明示確認する責務を port 側へ委譲する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddGpgBackupSpareCommand {
    pub unwrap_serial: Option<u32>,
    pub spare_serial: Option<u32>,
    pub assume_overwrite: bool,
}

impl AddGpgBackupSpareCommand {
    /// 明示指定された unwrap 機と spare 機が同一でないことを事前確認する。
    ///
    /// 両 serial が利用者入力で既に確定している場合、device open の前に domain invariant として拒否し、
    /// 同一 device の recipient を二重登録する経路を作らない。
    pub fn ensure_requested_serials_distinct(&self) -> Result<()> {
        if self.unwrap_serial.is_some() && self.unwrap_serial == self.spare_serial {
            return Err(invalid_input(
                "unwrap YubiKey serial and spare YubiKey serial must be different",
            )
            .into());
        }
        Ok(())
    }

    /// 解決済み unwrap 機と spare 機が別 device を指すことを確認する。
    pub fn ensure_distinct_resolved_serials(
        &self,
        unwrap_serial: u32,
        spare_serial: u32,
    ) -> Result<()> {
        if unwrap_serial == spare_serial {
            return Err(invalid_input(
                "unwrap YubiKey serial and spare YubiKey serial must be different",
            )
            .into());
        }
        Ok(())
    }
}

/// password-store-remote の provisioning（保管側 create/update）use case の入力 command。
///
/// 非対話実行での明示上書き許可、および `--url` で明示指定された clone URL 文字列の有無を保持する。
/// clone URL は private repo の SSH clone URL であって秘密情報ではないため、argv（`--url`）に載せてよい。
/// `url` が `None` の場合だけ application が port 経由で可視プロンプト（対話）または pipe（非対話）から
/// 1 行を読む。値そのものの形式検証は domain rule [`PasswordStoreRemote::parse`] に委ねる。対話実行では
/// 上書き対象 secret name と project name を表示して明示確認する責務を port 側へ委譲する。
///
/// この command は YubiKey storage を読まないため device serial を保持しない。BWS 登録・更新に使う
/// access token は hidden prompt / pipe から取得する credential であり、その取得は application が
/// `BwsAccessTokenInputPort` 経由で行う。YubiKey へ保存する `bws-access-token` は別経路の復旧用最小権限
/// token であり、この provisioning 入力と同一値にする運用を前提にしない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionPasswordStoreRemoteCommand {
    pub assume_overwrite: bool,
    pub url: Option<String>,
}

fn invalid_input(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests {
    use super::VerifyYubikeyCommand;
    use crate::domain::verification::{CheckName, ExternalCheck};

    fn verify_command(checks: Vec<ExternalCheck>, all: bool) -> VerifyYubikeyCommand {
        VerifyYubikeyCommand {
            serial: None,
            checks,
            all,
            email_override: None,
        }
    }

    /// 同じ check を複数回指定しても各 external check は 1 回だけ実行する（bw-login の二重実行による
    /// 2 回目 `bw login` の「already logged in」失敗を防ぐ）。
    #[test]
    fn requested_external_checks_dedupes_repeated_check() {
        let command = verify_command(vec![ExternalCheck::BwLogin, ExternalCheck::BwLogin], false);
        assert_eq!(
            command.requested_external_checks().expect("checks"),
            vec![CheckName::BwLogin]
        );
    }

    /// 混在した重複でも出現順を保ったまま重複排除する。
    #[test]
    fn requested_external_checks_dedupes_mixed_repeats_preserving_order() {
        let command = verify_command(
            vec![
                ExternalCheck::BwLogin,
                ExternalCheck::Bws,
                ExternalCheck::BwLogin,
                ExternalCheck::Bws,
            ],
            false,
        );
        assert_eq!(
            command.requested_external_checks().expect("checks"),
            vec![CheckName::BwLogin, CheckName::Bws]
        );
    }

    /// `--all` と `--check` の併用は不変条件違反として失敗する。
    #[test]
    fn requested_external_checks_rejects_all_with_check() {
        let command = verify_command(vec![ExternalCheck::BwLogin], true);
        assert!(command.requested_external_checks().is_err());
    }
}
