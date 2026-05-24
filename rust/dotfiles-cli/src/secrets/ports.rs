//! `dotfiles secrets` application 層が外部境界へ要求する port。
//!
//! application はこの module の trait だけに依存し、実機 YubiKey と test stub の具体的な
//! 入出力差分は adapter 側に閉じる。

use crate::Result;

use super::domain::{PivObjectId, SecretName};
use super::support::protection::{InterruptGuard, ProtectedSecret, SecretSession};

/// application/storage 操作が要求する最小の YubiKey device capability 契約。
pub(crate) trait SecretDevice {
    fn serial(&self) -> u32;
    fn key_exists(&mut self) -> Result<bool>;
    fn check_key_generation_preconditions(&mut self) -> Result<()>;
    fn check_management_auth_preconditions(&mut self) -> Result<()>;
    fn generate_key(&mut self) -> Result<()>;
    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>>;
    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()>;
    fn wrap_key(&mut self, key: &[u8]) -> Result<Vec<u8>>;
    fn requires_pin_input(&self) -> bool;
    fn verify_pin(&mut self, pin: &[u8]) -> Result<()>;
    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Vec<u8>>;
}

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
/// 実機 adapter と test stub は同じ入力順序と device 操作順序をこの trait で共有する。
/// 非対話条件や利用者向け error contract は application 層が所有する。
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
    fn prompt_yes_no(&mut self, prompt: &str, interrupt: &InterruptGuard) -> Result<bool>;
}
