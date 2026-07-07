//! process / terminal / report 出力を I/O port 契約へ接続する adapter。
//!
//! process-generic な stdin/stdout、prompt、JSON report 変換を扱い、YubiKey PIV device 操作や
//! Bitwarden vault SDK 呼び出しは持たない。

#[cfg(feature = "secrets-internal-test-stub")]
mod internal_stub;
#[cfg(not(feature = "secrets-internal-test-stub"))]
mod process;
mod report;

use crate::{
    Result,
    domain::{
        enrollment::EnrollSummary,
        gpg_restore::{OpenSshPublicKey, RestoreGpgSummary},
        pass_restore::RestorePassSummary,
        verification::VerifySummary,
    },
    ports::io::{
        PasswordStoreRemoteInputPort, PinInputPort, ReportPort, SecretInputPort, SecretOutputPort,
        SshPublicKeyOutputPort,
    },
    support::protection::ProtectedSecret,
};

/// process I/O と secret 入出力を port 契約へ翻訳する adapter。
#[cfg(feature = "secrets-internal-test-stub")]
type ProcessIoBackend = internal_stub::ProcessIoAdapter;
#[cfg(not(feature = "secrets-internal-test-stub"))]
type ProcessIoBackend = process::ProcessIoAdapter;

#[derive(Default)]
pub(crate) struct ProcessIoAdapter(ProcessIoBackend);

impl PinInputPort for ProcessIoAdapter {
    fn read_pin(&self) -> Result<ProtectedSecret> {
        self.0.read_pin()
    }
}

impl SecretInputPort for ProcessIoAdapter {
    fn read_bitwarden_client_id_secret(&self) -> Result<ProtectedSecret> {
        self.0.read_bitwarden_client_id_secret()
    }

    fn read_bitwarden_client_secret(&self) -> Result<ProtectedSecret> {
        self.0.read_bitwarden_client_secret()
    }

    fn read_bitwarden_master_password(&self) -> Result<ProtectedSecret> {
        self.0.read_bitwarden_master_password()
    }
}

impl PasswordStoreRemoteInputPort for ProcessIoAdapter {
    fn read_password_store_remote_url(&self) -> Result<String> {
        self.0.read_password_store_remote_url()
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

    fn write_restore_gpg_report(&self, summary: &RestoreGpgSummary) -> Result<()> {
        self.0.write_restore_gpg_report(summary)
    }

    fn write_restore_pass_report(&self, summary: &RestorePassSummary) -> Result<()> {
        self.0.write_restore_pass_report(summary)
    }
}
