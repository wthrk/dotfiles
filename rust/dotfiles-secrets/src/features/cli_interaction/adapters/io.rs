//! Process/terminal/report ports の forwarding-only adapter。

use crate::{
    Result,
    features::cli_interaction::ports::io::{
        BackupUpdateConfirmationPort, BitwardenClientSecretInputPort, BootstrapDocumentInputPort,
        ClockPort, PasswordStoreRemoteInputPort, ReportPort, RotationContinuationPort,
        SecretStorageStatusOutputPort, SshPublicKeyOutputPort,
    },
    features::cli_interaction::presentation::io::{
        HiddenBootstrapDocumentInput, HiddenTokenInput, JsonReport, ProcessPresentation,
        StreamedBootstrapDocumentInput, StreamedTokenInput,
    },
    features::cli_interaction::support::clock::{self, SystemClock},
    features::{
        gpg_backup_recovery::ports::public::{OpenSshPublicKey, RestoreGpgSummary},
        password_store::ports::public::RestorePassSummary,
        provisioning_verification::ports::public::{EnrollSummary, VerifySummary},
        yubikey_lifecycle::ports::public::{BootstrapSecretDocumentInput, SecretStorageStatus},
    },
    foundation::protection::ProtectedSecret,
};

impl BitwardenClientSecretInputPort for HiddenTokenInput {
    fn read_bitwarden_client_secret(&self) -> Result<ProtectedSecret> {
        HiddenTokenInput::read_bitwarden_client_secret(self)
    }
}
impl BitwardenClientSecretInputPort for StreamedTokenInput {
    fn read_bitwarden_client_secret(&self) -> Result<ProtectedSecret> {
        StreamedTokenInput::read_bitwarden_client_secret(self)
    }
}
impl BootstrapDocumentInputPort for HiddenBootstrapDocumentInput {
    fn read_bootstrap_secret_document_input(&mut self) -> Result<BootstrapSecretDocumentInput> {
        HiddenBootstrapDocumentInput::read(self)
    }
}
impl BootstrapDocumentInputPort for StreamedBootstrapDocumentInput {
    fn read_bootstrap_secret_document_input(&mut self) -> Result<BootstrapSecretDocumentInput> {
        StreamedBootstrapDocumentInput::read(self)
    }
}
impl PasswordStoreRemoteInputPort for ProcessPresentation {
    fn read_password_store_remote_url(&self) -> Result<String> {
        ProcessPresentation::read_password_store_remote_url(self)
    }
}
impl RotationContinuationPort for ProcessPresentation {
    fn continue_rotation(&self) -> Result<bool> {
        ProcessPresentation::continue_rotation(self)
    }
}
impl SecretStorageStatusOutputPort for ProcessPresentation {
    fn write_secret_storage_status(&self, status: &SecretStorageStatus) -> Result<()> {
        ProcessPresentation::write_secret_storage_status(self, status)
    }
}
impl SshPublicKeyOutputPort for ProcessPresentation {
    fn write_ssh_public_key(&self, public_key: &OpenSshPublicKey) -> Result<()> {
        ProcessPresentation::write_ssh_public_key(self, public_key)
    }
}
impl ClockPort for SystemClock {
    fn now_rfc3339_utc(&self) -> Result<String> {
        clock::now_rfc3339_utc()
    }
}
impl BackupUpdateConfirmationPort for ProcessPresentation {
    fn confirm_backup_update(
        &self,
        project_name: &str,
        secret_name: &str,
        primary_fingerprint: &str,
        assume_overwrite: bool,
    ) -> Result<bool> {
        ProcessPresentation::confirm_backup_update(
            self,
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
        ProcessPresentation::confirm_secret_overwrite(
            self,
            project_name,
            secret_name,
            assume_overwrite,
        )
    }
}
impl ReportPort for JsonReport {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()> {
        JsonReport::write_enroll(self, summary)
    }
    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        JsonReport::write_verify(self, summary)
    }
    fn write_restore_gpg_report(&self, summary: &RestoreGpgSummary) -> Result<()> {
        JsonReport::write_restore_gpg(self, summary)
    }
    fn write_restore_pass_report(&self, summary: &RestorePassSummary) -> Result<()> {
        JsonReport::write_restore_pass(self, summary)
    }
}
