//! `dotfiles secrets` application 層が外部境界へ要求する port。
//!
//! application はこの module の trait だけに依存し、実機 YubiKey と test stub の具体的な
//! 入出力差分は adapter 側に閉じる。

use crate::Result;
use anyhow::bail;

use super::{
    domain::{self, SecretName},
    support::protection::{InterruptGuard, ProtectedSecret, SecretSession},
};

pub(crate) const SPARE_SERIAL_NONINTERACTIVE_ERROR: &str =
    "pass --spare-serial in non-interactive use";
pub(crate) const SECRET_STDOUT_TERMINAL_ERROR: &str =
    "refusing to write secret to terminal; redirect stdout to a file or pipe";

/// 登録に必要な 3 field を同じ保護 session で所有する。
pub(crate) struct EnrollmentSecretSet<'session> {
    pub(crate) bw_email: ProtectedSecret<'session>,
    pub(crate) bw_password: ProtectedSecret<'session>,
    pub(crate) bws_access_token: ProtectedSecret<'session>,
}

impl<'session> EnrollmentSecretSet<'session> {
    /// 同じ `SecretSession` に所属する 3 field から 登録対象 secretを構築する。
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
///
/// 実機 adapter と test stub は同じ非対話条件、入力順序、device 操作順序をこの trait で共有する。
pub(crate) trait SecretsBoundary {
    type Device: domain::SecretDevice;

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
    fn prompt_yes_no(&mut self, prompt: &str) -> Result<bool>;
    fn require_serial_for_noninteractive(&self, serial: Option<u32>) -> Result<()> {
        if serial.is_none() && !self.stdin_is_terminal() {
            bail!("pass --serial in non-interactive use");
        }
        Ok(())
    }
    fn require_spare_serial_for_noninteractive(&self, spare_serial: Option<u32>) -> Result<()> {
        if spare_serial.is_none() && !self.stdin_is_terminal() {
            bail!(SPARE_SERIAL_NONINTERACTIVE_ERROR);
        }
        Ok(())
    }
    fn require_primary_serial_for_noninteractive(&self, primary_serial: Option<u32>) -> Result<()> {
        if primary_serial.is_none() && !self.stdin_is_terminal() {
            bail!("pass --primary-serial in non-interactive use");
        }
        Ok(())
    }
    fn require_stdin_for_noninteractive(
        &self,
        enabled: bool,
        option_name: &'static str,
    ) -> Result<()> {
        if !enabled && !self.stdin_is_terminal() {
            bail!("pass {option_name} in non-interactive use");
        }
        Ok(())
    }
    fn require_secret_stdout_target(&self) -> Result<()> {
        if self.stdout_is_terminal() {
            bail!(SECRET_STDOUT_TERMINAL_ERROR);
        }
        Ok(())
    }
}
