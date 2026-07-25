//! YubiKey lifecycle command が公開する non-secret diagnostic contract。

use crate::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiagnosticCommand {
    EnrollPrimary,
    EnrollSpare,
    ProvisionBwsToken,
}

pub(crate) trait DiagnosticScopeControl {
    fn begin(&self, enabled: bool, command: DiagnosticCommand) -> Box<dyn DiagnosticRunToken + '_>;
}

pub(crate) trait DiagnosticRunToken {
    fn finish(self: Box<Self>, result: &Result<()>);
}
