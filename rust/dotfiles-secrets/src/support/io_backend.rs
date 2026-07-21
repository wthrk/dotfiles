//! Process/terminal/report concrete backend operations.
//!
//! This module owns all terminal selection, prompt wording, JSON decoding and report
//! serialization.  Adapters only forward the corresponding port calls here.

use std::collections::BTreeMap;

use crate::{
    Result,
    domain::{
        enrollment::EnrollSummary,
        gpg_restore::{OpenSshPublicKey, RestoreGpgSummary},
        manifest::BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT,
        pass_restore::RestorePassSummary,
        storage::SecretStorageStatus,
        verification::VerifySummary,
    },
    support::{clock, process_io, protection::ProtectedSecret, report},
};

pub(crate) fn read_bitwarden_client_secret_secret() -> Result<ProtectedSecret> {
    process_io::read_hidden_line(
        "bitwarden-client-secret: ",
        16 * 1024,
        "hidden secret input is too large",
    )
}

pub(crate) fn read_streamed_secret() -> Result<ProtectedSecret> {
    process_io::read_stdin_line(16 * 1024, "stdin secret input is too large")
}

pub(crate) fn read_piv_pin_secret() -> Result<ProtectedSecret> {
    process_io::read_hidden_tty_line("YubiKey PIV PIN: ", 64, "YubiKey PIV PIN is too large")
}

pub(crate) fn read_password_store_remote_url() -> Result<String> {
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

pub(crate) fn continue_rotation() -> Result<bool> {
    if !process_io::stdin_is_terminal() {
        return Ok(false);
    }
    let answer = process_io::read_control_line("rotate another YubiKey? [y/N]: ")?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "YES" | "Yes"))
}

pub(crate) fn read_bootstrap_secret_fields() -> Result<BTreeMap<String, ProtectedSecret>> {
    process_io::read_stdin_all(64 * 1024, "bootstrap secret JSON input is too large")?
        .decode_json_string_map(BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT)
}

pub(crate) fn write_secret_storage_status(status: &SecretStorageStatus) -> Result<()> {
    for name in status.stored() {
        println!("{name}");
    }
    Ok(())
}

pub(crate) fn write_ssh_public_key(public_key: &OpenSshPublicKey) -> Result<()> {
    println!("{}", public_key.as_str());
    Ok(())
}

pub(crate) fn now_rfc3339_utc() -> Result<String> {
    clock::now_rfc3339_utc()
}

pub(crate) fn confirm_backup_update(
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

pub(crate) fn confirm_secret_overwrite(
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

pub(crate) fn write_enroll_report(summary: &EnrollSummary) -> Result<()> {
    report::write_enroll(summary)
}
pub(crate) fn write_verify_report(summary: &VerifySummary) -> Result<()> {
    report::write_verify(summary)
}
pub(crate) fn write_restore_gpg_report(summary: &RestoreGpgSummary) -> Result<()> {
    report::write_restore_gpg(summary)
}
pub(crate) fn write_restore_pass_report(summary: &RestorePassSummary) -> Result<()> {
    report::write_restore_pass(summary)
}
