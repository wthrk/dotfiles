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

/// verify-yubikey use case の入力 command。
///
/// 要求 check と `--all` 指定だけを保持し、device 選択手段は port 境界へ委譲する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyYubikeyCommand {
    pub checks: Vec<ExternalCheck>,
    pub all: bool,
}

impl VerifyYubikeyCommand {
    /// verify-yubikey が要求された external check 集合を domain check 名へ正規化する。
    ///
    /// `--all` と `--check` の併用は不変条件違反として失敗する。
    /// 同じ check を複数回指定しても各 external check は高々 1 回だけ実行する。
    pub fn requested_external_checks(&self) -> Result<Vec<CheckName>> {
        if self.all && !self.checks.is_empty() {
            return Err(invalid_input("--all and --check cannot be used together").into());
        }

        if self.all {
            return Ok(vec![CheckName::Vault]);
        }

        // 出現順を保ったまま重複を取り除き、同じ external check を 2 回以上実行しない。
        let mut checks = Vec::new();
        for check in &self.checks {
            let name = match check {
                ExternalCheck::Vault => CheckName::Vault,
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
/// Bitwarden 個人 vault へ接続するための資格情報読み出し手段や
/// envelope 取得手段は port 境界へ委譲する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestoreGpgCommand;

/// restore-pass use case の入力 command。
///
/// Bitwarden 個人 vault へ接続するための資格情報読み出し手段や
/// remote URL 取得手段、clone 手段は port 境界へ委譲する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestorePassCommand;

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
/// Bitwarden 個人 vault への接続資格情報は YubiKey storage に保存済みの account API key から use case が
/// 局所的に読み出すため、この command は外部入力方式や device override 値を保持しない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProvisionPasswordStoreRemoteCommand;

fn invalid_input(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into())
}

/// CLI parse 後の command domain 値が external check 指定を正規化する規則を検証する。
#[cfg(test)]
mod tests {
    use super::VerifyYubikeyCommand;
    use crate::secrets::domain::verification::{CheckName, ExternalCheck};

    fn verify_command(checks: Vec<ExternalCheck>, all: bool) -> VerifyYubikeyCommand {
        VerifyYubikeyCommand { checks, all }
    }

    /// 重複した外部確認指定は use case へ渡す前に 1 回の check へ正規化する。
    #[test]
    fn requested_external_checks_dedupes_repeated_check() {
        let command = verify_command(vec![ExternalCheck::Vault, ExternalCheck::Vault], false);
        assert_eq!(
            command.requested_external_checks().expect("checks"),
            vec![CheckName::Vault]
        );
    }

    /// `--all` と `--check` の併用は不変条件違反として失敗する。
    #[test]
    fn requested_external_checks_rejects_all_with_check() {
        let command = verify_command(vec![ExternalCheck::Vault], true);
        assert!(command.requested_external_checks().is_err());
    }
}
