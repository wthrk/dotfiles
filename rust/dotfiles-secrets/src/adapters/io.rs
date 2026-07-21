//! process / terminal / report I/O を port 契約へ翻訳する adapter。
//!
//! この source は port trait implementation だけを持つ。receiver の marker と process-generic
//! primitive は `support` が所有し、adapter 自身は wrapper、state、helper、inherent impl を持たない。

use std::collections::BTreeMap;

use crate::{
    Result,
    domain::{
        enrollment::EnrollSummary,
        gpg_restore::{OpenSshPublicKey, RestoreGpgSummary},
        manifest::BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT,
        pass_restore::RestorePassSummary,
        verification::VerifySummary,
    },
    ports::io::{
        BackupUpdateConfirmationPort, BootstrapSecretDocumentInputPort, ClockPort,
        PasswordStoreRemoteInputPort, PivPinInputPort, ReportPort, RotationContinuationPort,
        SecretInputPort, SecretStorageStatusOutputPort, SshPublicKeyOutputPort,
    },
    support::{
        adapter_backend::{JsonReportBackend, ProcessIoBackend},
        clock, process_io,
        protection::ProtectedSecret,
        report,
    },
};

impl SecretInputPort for ProcessIoBackend {
    fn read_bw_email_secret(&self) -> Result<ProtectedSecret> {
        process_io::read_visible_line("bw-email: ", 16 * 1024, "visible secret input is too large")
    }

    fn read_bw_password_secret(&self) -> Result<ProtectedSecret> {
        process_io::read_hidden_line(
            "bw-password: ",
            16 * 1024,
            "hidden secret input is too large",
        )
    }

    fn read_bitwarden_client_id_secret(&self) -> Result<ProtectedSecret> {
        process_io::read_hidden_line(
            "bitwarden-client-id: ",
            16 * 1024,
            "hidden secret input is too large",
        )
    }

    fn read_bitwarden_client_secret_secret(&self) -> Result<ProtectedSecret> {
        process_io::read_hidden_line(
            "bitwarden-client-secret: ",
            16 * 1024,
            "hidden secret input is too large",
        )
    }

    fn read_streamed_secret(&self) -> Result<ProtectedSecret> {
        process_io::read_stdin_line(16 * 1024, "stdin secret input is too large")
    }
}

impl PivPinInputPort for ProcessIoBackend {
    /// PIV 管理操作でだけ使う PIN を hidden TTY prompt から保護値として取得する。
    ///
    /// Primary sources:
    /// - Yubico PIV PIN-only mode, `PIN-protected` / `Management key authentication`:
    ///   https://docs.yubico.com/yesdk/users-manual/application-piv/pin-only.html#pin-protected
    ///   https://docs.yubico.com/yesdk/users-manual/application-piv/pin-only.html#management-key-authentication
    /// - yubikey 0.9.0-pre.0 `YubiKey::verify_pin`:
    ///   https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/yubikey.rs
    fn read_piv_pin_secret(&self) -> Result<ProtectedSecret> {
        process_io::read_hidden_tty_line("YubiKey PIV PIN: ", 64, "YubiKey PIV PIN is too large")
    }
}

impl PasswordStoreRemoteInputPort for ProcessIoBackend {
    fn read_password_store_remote_url(&self) -> Result<String> {
        if process_io::stdin_is_terminal() {
            process_io::read_visible_plain_line(
                "password-store-remote: ",
                16 * 1024,
                "password-store-remote input is too large",
            )
        } else {
            process_io::read_stdin_plain_line(16 * 1024, "password-store-remote input is too large")
        }
    }
}

impl RotationContinuationPort for ProcessIoBackend {
    fn continue_rotation(&self) -> Result<bool> {
        if !process_io::stdin_is_terminal() {
            return Ok(false);
        }
        let answer = process_io::read_control_line("rotate another YubiKey? [y/N]: ")?;
        Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "YES" | "Yes"))
    }
}

impl BootstrapSecretDocumentInputPort for ProcessIoBackend {
    fn read_bootstrap_secret_fields(&self) -> Result<BTreeMap<String, ProtectedSecret>> {
        process_io::read_stdin_all(64 * 1024, "bootstrap secret JSON input is too large")?
            .decode_json_string_map(BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT)
    }
}

impl SecretStorageStatusOutputPort for ProcessIoBackend {
    fn write_secret_storage_status(
        &self,
        status: &crate::domain::storage::SecretStorageStatus,
    ) -> Result<()> {
        for name in status.stored() {
            println!("{name}");
        }
        Ok(())
    }
}

impl SshPublicKeyOutputPort for ProcessIoBackend {
    fn write_ssh_public_key(&self, public_key: &OpenSshPublicKey) -> Result<()> {
        println!("{}", public_key.as_str());
        Ok(())
    }
}

impl ClockPort for ProcessIoBackend {
    fn now_rfc3339_utc(&self) -> Result<String> {
        clock::now_rfc3339_utc()
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
        if !process_io::stdin_is_terminal() {
            return Ok(assume_overwrite);
        }
        let answer = process_io::read_control_line(&format!(
            "update BWS secret {secret_name} in project {project_name} (primary fingerprint {primary_fingerprint})? [y/N]: "
        ))?;
        Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "YES" | "Yes"))
    }

    fn confirm_secret_overwrite(
        &self,
        project_name: &str,
        secret_name: &str,
        assume_overwrite: bool,
    ) -> Result<bool> {
        if !process_io::stdin_is_terminal() {
            return Ok(assume_overwrite);
        }
        let answer = process_io::read_control_line(&format!(
            "update BWS secret {secret_name} in project {project_name}? [y/N]: "
        ))?;
        Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "YES" | "Yes"))
    }
}

impl ReportPort for JsonReportBackend {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()> {
        report::write_enroll(summary)
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        report::write_verify(summary)
    }

    fn write_restore_gpg_report(&self, summary: &RestoreGpgSummary) -> Result<()> {
        report::write_restore_gpg(summary)
    }

    fn write_restore_pass_report(&self, summary: &RestorePassSummary) -> Result<()> {
        report::write_restore_pass(summary)
    }
}
