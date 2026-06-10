//! `dotfiles secrets` use case の入力 command を表す domain model。
//!
//! CLI option の parse 方式や prompt 手段は含めず、application が適用する対象、
//! 上書き可否、外部 check 指定だけを保持する。

use anyhow::Result;

use super::{
    piv::{SecretName, SecretStorageSpec},
    verification::{CheckName, ExternalCheck},
};

/// setup use case の入力 command。
///
/// device 選択手段や prompt 方針は含めない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupCommand;

/// put use case の入力 command。
///
/// 対象 secret と既存値上書き可否という domain 意味だけを保持する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PutCommand {
    pub name: SecretName,
    pub force: bool,
}

impl PutCommand {
    /// 指定 serial に対する put 対象の storage spec を返す。
    pub fn storage_spec(&self, serial: u32) -> SecretStorageSpec {
        self.name.storage_spec(serial)
    }
}

/// get use case の入力 command。
///
/// 取得対象 secret だけを保持し、出力形式は含めない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GetCommand {
    pub name: SecretName,
}

impl GetCommand {
    /// 指定 serial に対する get 対象の storage spec を返す。
    pub fn storage_spec(&self, serial: u32) -> SecretStorageSpec {
        self.name.storage_spec(serial)
    }
}

/// enroll-primary use case の入力 command。
///
/// primary 登録の入力境界を表す。対象 YubiKey 選択は port 境界で行う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnrollPrimaryCommand;

/// enroll-spare use case の入力 command。
///
/// primary と spare の選択フローは含めない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnrollSpareCommand;

/// rotate-bws-token use case の入力 command。
///
/// 保存し直す secret 名を固定する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RotateBwsTokenCommand;

impl RotateBwsTokenCommand {
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
/// 要求 check、`--all` 指定、bw-login 外部確認の `--email` override の有無を保持し、
/// device 選択手段は port 境界へ委譲する。`email_override` が `Some` のときだけ bw-login 外部確認で
/// YubiKey の `bw-email` を使わず override を使う（yubikey-secret-storage-design.md の `dotfiles secrets verify-yubikey` 節）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyYubikeyCommand {
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
/// `bws-access-token` を読み出す device 選択手段や
/// envelope 取得手段は port 境界へ委譲する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestoreGpgCommand;

/// restore-pass use case の入力 command。
///
/// `bws-access-token` を読み出す device 選択手段や
/// remote URL 取得手段、clone 手段は port 境界へ委譲する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestorePassCommand;

/// bw-login use case の入力 command。
///
/// `bw-email` / `bw-password` を読み出す対象と、`--email` override の有無だけを保持する。
/// device 選択手段、YubiKey secret 取得手順、OTP 入力手段、`bw` CLI 実行詳細は port 境界へ委譲する。
/// `email_override` が `Some` のときだけ YubiKey の `bw-email` を使わず override を使う（spec L178）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwLoginCommand {
    pub email_override: Option<String>,
}

/// export-ssh-public-key use case の入力 command。
///
/// 対象鍵の決定は application が password-store と keyring の既存状態から行うため、override 値は持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportSshPublicKeyCommand;

/// gpg-secret-key-backup の primary 登録 use case の入力 command。
///
/// 対象 primary の決定は application が password-store と keyring の既存状態から行うため、override 値は持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisterGpgBackupCommand;

/// password-store-remote の provisioning（保管側 create/use）use case の入力 command。
///
/// この command は外部入力方式や入力値を保持しない。`password-store-remote` の値取得は
/// `PasswordStoreRemoteInputPort` に委譲し、値そのものの形式検証は domain rule
/// [`PasswordStoreRemote::parse`](crate::secrets::domain::pass_restore::PasswordStoreRemote::parse)
/// に委ねる。
///
/// YubiKey storage を読まないため device serial も保持しない。BWS 登録に使う access token は
/// credential であり、その取得は application が `BwsAccessTokenInputPort` 経由で行う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProvisionPasswordStoreRemoteCommand;

fn invalid_input(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests {
    use super::VerifyYubikeyCommand;
    use crate::secrets::domain::verification::{CheckName, ExternalCheck};

    fn verify_command(checks: Vec<ExternalCheck>, all: bool) -> VerifyYubikeyCommand {
        VerifyYubikeyCommand {
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
