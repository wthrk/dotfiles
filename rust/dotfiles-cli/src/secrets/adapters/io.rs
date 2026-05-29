//! process / terminal / report I/O を port 契約へ接続する adapter。
//!
//! この module は user interaction と JSON report 出力の翻訳だけを担い、YubiKey/BWS backend へ
//! 依存しない。

mod process;
mod report;

use crate::{
    Result,
    secrets::{
        domain::summary::{EnrollSummary, VerifySummary},
        ports::io::{
            BootstrapSecretDocumentInputPort, PinInputPort, ReportPort, RotationContinuationPort,
            SecretInputPort, SecretOutputPort,
        },
        support::protection::ProtectedSecret,
    },
};

/// process I/O と secret 入出力を port 契約へ翻訳する adapter。
#[derive(Default)]
pub(crate) struct ProcessIoAdapter(process::ProcessIoAdapter);

impl PinInputPort for ProcessIoAdapter {
    fn read_pin(&self) -> Result<ProtectedSecret> {
        self.0.read_pin()
    }
}

impl SecretInputPort for ProcessIoAdapter {
    fn read_bw_email_secret(&self) -> Result<ProtectedSecret> {
        self.0.read_bw_email_secret()
    }

    fn read_bw_password_secret(&self) -> Result<ProtectedSecret> {
        self.0.read_bw_password_secret()
    }

    fn read_bws_access_token_secret(&self) -> Result<ProtectedSecret> {
        self.0.read_bws_access_token_secret()
    }

    fn read_streamed_secret(&self) -> Result<ProtectedSecret> {
        self.0.read_streamed_secret()
    }
}

impl RotationContinuationPort for ProcessIoAdapter {
    fn continue_rotation(&self) -> Result<bool> {
        self.0.continue_rotation()
    }
}

impl BootstrapSecretDocumentInputPort for ProcessIoAdapter {
    fn read_bootstrap_secret_fields(
        &self,
    ) -> Result<std::collections::BTreeMap<String, ProtectedSecret>> {
        self.0.read_bootstrap_secret_fields()
    }
}

impl SecretOutputPort for ProcessIoAdapter {
    fn write_secret(&self, secret: &ProtectedSecret) -> Result<()> {
        self.0.write_secret(secret)
    }
}

/// CLI JSON report 出力を port 契約へ翻訳する adapter。
#[derive(Default)]
pub(crate) struct JsonReportAdapter(report::JsonReportAdapter);

impl ReportPort for JsonReportAdapter {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()> {
        self.0.write_enroll_report(summary)
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        self.0.write_verify_report(summary)
    }
}
