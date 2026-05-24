//! `dotfiles secrets` の application/adapter 間境界。
//!
//! use case の順序制御が要求する外部 I/O 契約だけをここへ固定し、application 実装と
//! adapter 実装の依存方向を安定させる。

use std::collections::BTreeMap;

use crate::Result;

use super::{
    domain::SecretName,
    ports::SecretDevice,
    support::protection::{InterruptGuard, ProtectedSecret, SecretSession},
};

/// 登録に必要な 3 field を同じ保護 session で所有する。
pub(crate) struct EnrollmentSecretSet<'session> {
    pub(crate) bw_email: ProtectedSecret<'session>,
    pub(crate) bw_password: ProtectedSecret<'session>,
    pub(crate) bws_access_token: ProtectedSecret<'session>,
}

impl<'session> EnrollmentSecretSet<'session> {
    /// 同じ `SecretSession` に所属する 3 field から 登録対象 secret を構築する。
    pub(crate) fn new(
        bw_email: ProtectedSecret<'session>,
        bw_password: ProtectedSecret<'session>,
        bws_access_token: ProtectedSecret<'session>,
    ) -> Self {
        Self {
            bw_email,
            bw_password,
            bws_access_token,
        }
    }

    #[cfg(test)]
    pub(crate) fn assert_secret_eq(&self, name: SecretName, expected: &[u8]) {
        match name {
            SecretName::BwEmail => self
                .bw_email
                .with_secret(|secret| assert_eq!(secret, expected)),
            SecretName::BwPassword => self
                .bw_password
                .with_secret(|secret| assert_eq!(secret, expected)),
            SecretName::BwsAccessToken => self
                .bws_access_token
                .with_secret(|secret| assert_eq!(secret, expected)),
        }
    }
}

/// application use case が利用する外部 I/O 境界。
pub(crate) trait SecretsBoundary {
    type Device: SecretDevice;

    fn stdin_is_terminal(&self) -> bool;
    fn stdout_is_terminal(&self) -> bool;
    fn open_device(&mut self, serial: Option<u32>) -> Result<Self::Device>;
    fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
        interrupt: &InterruptGuard,
    ) -> Result<Self::Device>;
    fn read_enrollment_secret_set<'session>(
        &mut self,
        stdin_json: bool,
        memory: &'session SecretSession,
    ) -> Result<EnrollmentSecretSet<'session>>;
    fn read_secret_for_put<'session>(
        &mut self,
        name: SecretName,
        stdin: bool,
        memory: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>>;
    fn read_yubikey_pin<'session>(
        &mut self,
        memory: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>>;
    fn write_secret_output(&mut self, secret: &[u8]) -> Result<()>;
    fn prompt_yes_no(&mut self, prompt: &str, interrupt: &InterruptGuard) -> Result<bool>;
}

/// summary に出す確認項目の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub(crate) enum CheckStatus {
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "skipped")]
    Skipped,
}

/// YubiKey を primary と spare のどちらとして登録したかを表す role。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum YubikeyRole {
    Primary,
    Spare,
}

/// summary JSON の `checks` key として使う閉じた確認項目名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
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

/// enroll 系 command の成功 summary。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct EnrollSummary {
    pub(crate) serial: u32,
    pub(crate) role: YubikeyRole,
    pub(crate) checks: BTreeMap<CheckName, CheckStatus>,
}

/// verify 系 command の成功 summary。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct VerifySummary {
    pub(crate) serial: u32,
    pub(crate) checks: BTreeMap<CheckName, CheckStatus>,
}
