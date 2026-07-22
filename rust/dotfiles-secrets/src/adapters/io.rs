//! Process/terminal/report ports の forwarding-only adapter。

use crate::{
    Result,
    domain::{
        enrollment::EnrollSummary,
        gpg_restore::{OpenSshPublicKey, RestoreGpgSummary},
        pass_restore::RestorePassSummary,
        storage::SecretStorageStatus,
        verification::VerifySummary,
    },
    ports::io::{
        BackupUpdateConfirmationPort, BitwardenClientSecretInputPort, BootstrapDocumentInputPort,
        ClockPort, PasswordStoreRemoteInputPort, PivPinInputPort, ReportPort,
        RotationContinuationPort, SecretStorageStatusOutputPort, SshPublicKeyOutputPort,
    },
    support::{
        adapter_backend::{
            HiddenBootstrapDocumentInputBackend, HiddenTokenInputBackend, JsonReportBackend,
            ProcessIoBackend, StreamedBootstrapDocumentInputBackend, StreamedTokenInputBackend,
        },
        io_backend,
        protection::ProtectedSecret,
    },
};

impl BitwardenClientSecretInputPort for HiddenTokenInputBackend {
    fn read_bitwarden_client_secret(&self) -> Result<ProtectedSecret> {
        io_backend::read_bitwarden_client_secret_tty_secret()
    }
}
impl BitwardenClientSecretInputPort for StreamedTokenInputBackend {
    fn read_bitwarden_client_secret(&self) -> Result<ProtectedSecret> {
        io_backend::read_streamed_secret()
    }
}
impl BootstrapDocumentInputPort for HiddenBootstrapDocumentInputBackend {
    fn read_bootstrap_secret_document_input(
        &mut self,
    ) -> Result<crate::domain::manifest::BootstrapSecretDocumentInput> {
        io_backend::read_hidden_bootstrap_secret_document_input()
    }
}
impl BootstrapDocumentInputPort for StreamedBootstrapDocumentInputBackend {
    fn read_bootstrap_secret_document_input(
        &mut self,
    ) -> Result<crate::domain::manifest::BootstrapSecretDocumentInput> {
        io_backend::read_streamed_bootstrap_secret_document_input()
    }
}
impl PivPinInputPort for ProcessIoBackend {
    fn read_piv_pin_secret(&self) -> Result<ProtectedSecret> {
        io_backend::read_piv_pin_secret()
    }
}
impl PasswordStoreRemoteInputPort for ProcessIoBackend {
    fn read_password_store_remote_url(&self) -> Result<String> {
        io_backend::read_password_store_remote_url()
    }
}
impl RotationContinuationPort for ProcessIoBackend {
    fn continue_rotation(&self) -> Result<bool> {
        io_backend::continue_rotation()
    }
}
impl SecretStorageStatusOutputPort for ProcessIoBackend {
    fn write_secret_storage_status(&self, status: &SecretStorageStatus) -> Result<()> {
        io_backend::write_secret_storage_status(status)
    }
}
impl SshPublicKeyOutputPort for ProcessIoBackend {
    fn write_ssh_public_key(&self, public_key: &OpenSshPublicKey) -> Result<()> {
        io_backend::write_ssh_public_key(public_key)
    }
}
impl ClockPort for ProcessIoBackend {
    fn now_rfc3339_utc(&self) -> Result<String> {
        io_backend::now_rfc3339_utc()
    }
}
impl BackupUpdateConfirmationPort for ProcessIoBackend {
    fn confirm_backup_update(
        &self,
        project_name: &str,
        secret_name: &str,
        primary_fingerprint: &str,
        assume_overwrite: bool,
    ) -> Result<bool> {
        io_backend::confirm_backup_update(
            project_name,
            secret_name,
            primary_fingerprint,
            assume_overwrite,
        )
    }
    fn confirm_secret_overwrite(
        &self,
        project_name: &str,
        secret_name: &str,
        assume_overwrite: bool,
    ) -> Result<bool> {
        io_backend::confirm_secret_overwrite(project_name, secret_name, assume_overwrite)
    }
}
impl ReportPort for JsonReportBackend {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()> {
        io_backend::write_enroll_report(summary)
    }
    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        io_backend::write_verify_report(summary)
    }
    fn write_restore_gpg_report(&self, summary: &RestoreGpgSummary) -> Result<()> {
        io_backend::write_restore_gpg_report(summary)
    }
    fn write_restore_pass_report(&self, summary: &RestorePassSummary) -> Result<()> {
        io_backend::write_restore_pass_report(summary)
    }
}
