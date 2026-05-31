//! process / terminal / report 出力を I/O port 契約へ接続する adapter。
//!
//! process-generic な stdin/stdout、prompt、JSON report 変換を扱い、YubiKey PIV device 操作や
//! BWS SDK 呼び出しは持たない。

mod process;
mod report;

use crate::{
    Result,
    secrets::{
        domain::{
            enrollment::EnrollSummary,
            gpg_restore::{OpenSshPublicKey, RestoreGpgSummary},
            verification::VerifySummary,
        },
        ports::io::{
            BackupUpdateConfirmationPort, BootstrapSecretDocumentInputPort, ClockPort,
            PinInputPort, ReportPort, RotationContinuationPort, SecretInputPort, SecretOutputPort,
            SshPublicKeyOutputPort,
        },
        support::protection::ProtectedSecret,
    },
};

/// process I/O と secret 入出力を port 契約へ翻訳する adapter。
#[derive(Default)]
pub(in crate::secrets) struct ProcessIoAdapter(process::ProcessIoAdapter);

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

impl SshPublicKeyOutputPort for ProcessIoAdapter {
    fn write_ssh_public_key(&self, public_key: &OpenSshPublicKey) -> Result<()> {
        self.0.write_ssh_public_key(public_key)
    }
}

impl ClockPort for ProcessIoAdapter {
    fn now_rfc3339_utc(&self) -> Result<String> {
        self.0.now_rfc3339_utc()
    }
}

impl BackupUpdateConfirmationPort for ProcessIoAdapter {
    fn confirm_backup_update(
        &self,
        project_name: &str,
        secret_name: &str,
        primary_fingerprint: &str,
        assume_overwrite: bool,
    ) -> Result<bool> {
        self.0.confirm_backup_update(
            project_name,
            secret_name,
            primary_fingerprint,
            assume_overwrite,
        )
    }
}

/// CLI JSON report 出力を port 契約へ翻訳する adapter。
#[derive(Default)]
pub(in crate::secrets) struct JsonReportAdapter(report::JsonReportAdapter);

impl ReportPort for JsonReportAdapter {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()> {
        self.0.write_enroll_report(summary)
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        self.0.write_verify_report(summary)
    }

    fn write_restore_gpg_report(&self, summary: &RestoreGpgSummary) -> Result<()> {
        self.0.write_restore_gpg_report(summary)
    }
}
