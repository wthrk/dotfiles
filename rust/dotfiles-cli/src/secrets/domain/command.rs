//! `dotfiles secrets` use case command の domain model。
//!
//! CLI 表現や入出力手段ではなく、use case が扱う入力意味と不変条件だけを保持する。

use anyhow::Result;

use super::{
    piv::{SecretName, SecretStorageSpec},
    summary::CheckName,
};

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
}

fn invalid_input(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into())
}
