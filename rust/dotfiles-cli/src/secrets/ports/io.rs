//! secret 入出力と対話 prompt の外部境界。

use crate::Result;

use super::super::{
    application::EnrollmentSecretSet,
    domain::SecretName,
    support::protection::{InterruptGuard, ProtectedSecret, SecretSession},
};

/// 非対話判定に必要な terminal 状態を返す contract。
pub(crate) trait TerminalEnvironmentPort {
    fn stdin_is_terminal(&self) -> bool;
}

/// secret と PIN を保護済み値として取得する contract。
pub(crate) trait SecretInputPort {
    fn read_enrollment_secret_set<'session>(
        &mut self,
        stdin_json: bool,
        session: &'session SecretSession,
    ) -> Result<EnrollmentSecretSet<'session>>;
    fn read_secret_for_put<'session>(
        &mut self,
        name: SecretName,
        stdin: bool,
        session: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>>;
    fn read_yubikey_pin<'session>(
        &mut self,
        session: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>>;
}

/// yes/no prompt など terminal 上の短い対話を扱う contract。
pub(crate) trait TerminalPromptPort {
    fn prompt_yes_no(&mut self, prompt: &str, interrupt: &InterruptGuard) -> Result<bool>;
}

/// secret の stdout 出力と terminal 拒否を扱う contract。
pub(crate) trait SecretOutputPort {
    fn ensure_secret_stdout_target(&self) -> Result<()>;
    fn write_secret_output(&mut self, bytes: &[u8]) -> Result<()>;
}
