//! CLI 固有の入力文言、対話確認、stdout 表現、JSON report schema。
//!
//! process/TTY の byte I/O と clock は composition から注入された汎用関数へ委譲する。
//! この module は外部 SDK、device state、use case の成功条件を扱わない。

use crate::{
    Result,
    features::{
        gpg_backup_recovery::ports::public::{OpenSshPublicKey, RestoreGpgSummary},
        password_store::ports::public::RestorePassSummary,
        provisioning_verification::ports::public::{
            CheckName, CheckStatus, EnrollSummary, VerifySummary, YubikeyRole,
        },
        yubikey_lifecycle::ports::public::{
            BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT, BootstrapSecretDocumentInput,
            SecretStorageStatus,
        },
    },
    foundation::protection::ProtectedSecret,
};
use serde_json::json;

type HiddenSecretReader = fn(&str, usize, &'static str) -> Result<ProtectedSecret>;
type StreamedSecretReader = fn(usize, &'static str) -> Result<ProtectedSecret>;
type StreamedDocumentReader = fn(usize, &'static str) -> Result<ProtectedSecret>;
type TerminalProbe = fn() -> bool;
type ControlLineReader = fn(&str) -> Result<String>;
type VisibleLineReader = fn(&str, usize, &'static str) -> Result<String>;
type StreamedLineReader = fn(usize, &'static str) -> Result<String>;
type LineWriter = fn(&str) -> Result<()>;

/// controlling TTY の hidden BWS token input を port へ渡す presentation receiver。
///
/// 入力 schema・mask と TTY 選択だけを担い、token の保存、YubiKey 操作、BWS 呼出しは行わない。
pub(crate) struct HiddenTokenInput {
    read: HiddenSecretReader,
}

impl HiddenTokenInput {
    pub(crate) fn new(read: HiddenSecretReader) -> Self {
        Self { read }
    }

    pub(crate) fn read_bitwarden_client_secret(&self) -> Result<ProtectedSecret> {
        (self.read)(
            "bitwarden-client-secret: ",
            16 * 1024,
            "hidden secret input is too large",
        )
    }
}

/// pipe stdin の BWS token input を port へ渡す presentation receiver。
///
/// stdin byte stream の上限だけを適用し、secret の出力・保存・BWS 操作は行わない。
pub(crate) struct StreamedTokenInput {
    read: StreamedSecretReader,
}

impl StreamedTokenInput {
    pub(crate) fn new(read: StreamedSecretReader) -> Self {
        Self { read }
    }

    pub(crate) fn read_bitwarden_client_secret(&self) -> Result<ProtectedSecret> {
        (self.read)(16 * 1024, "stdin secret input is too large")
    }
}

/// controlling TTY の bootstrap document input を port へ渡す presentation receiver。
///
/// hidden input の表示形式と input schema だけを所有し、storage read の順序は決めない。
pub(crate) struct HiddenBootstrapDocumentInput {
    read: HiddenSecretReader,
}

impl HiddenBootstrapDocumentInput {
    pub(crate) fn new(read: HiddenSecretReader) -> Self {
        Self { read }
    }

    pub(crate) fn read(&self) -> Result<BootstrapSecretDocumentInput> {
        (self.read)(
            "bitwarden-client-secret: ",
            16 * 1024,
            "hidden secret input is too large",
        )
        .map(BootstrapSecretDocumentInput::BitwardenClientSecret)
    }
}

/// pipe stdin の bootstrap JSON input を decode する presentation receiver。
///
/// JSON schema decode とサイズ上限だけを担い、secret の用途・保存先・外部 I/O は決めない。
pub(crate) struct StreamedBootstrapDocumentInput {
    read: StreamedDocumentReader,
}

impl StreamedBootstrapDocumentInput {
    pub(crate) fn new(read: StreamedDocumentReader) -> Self {
        Self { read }
    }

    pub(crate) fn read(&self) -> Result<BootstrapSecretDocumentInput> {
        (self.read)(64 * 1024, "bootstrap secret JSON input is too large")?
            .decode_json_string_map(BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT)
            .map(BootstrapSecretDocumentInput::FieldMap)
    }
}

/// command の可視入力・確認・非secret出力を port へ渡す presentation receiver。
///
/// TTY/pipe 選択、prompt、JSON/report 表示だけを担い、use case 順序、SDK、device、clock は所有しない。
pub(crate) struct ProcessPresentation {
    stdin_is_terminal: TerminalProbe,
    read_control_line: ControlLineReader,
    read_visible_line: VisibleLineReader,
    read_streamed_line: StreamedLineReader,
    write_line: LineWriter,
}

impl ProcessPresentation {
    pub(crate) fn new(
        stdin_is_terminal: TerminalProbe,
        read_control_line: ControlLineReader,
        read_visible_line: VisibleLineReader,
        read_streamed_line: StreamedLineReader,
        write_line: LineWriter,
    ) -> Self {
        Self {
            stdin_is_terminal,
            read_control_line,
            read_visible_line,
            read_streamed_line,
            write_line,
        }
    }

    pub(crate) fn read_password_store_remote_url(&self) -> Result<String> {
        if (self.stdin_is_terminal)() {
            (self.read_visible_line)(
                "password-store-remote: ",
                16 * 1024,
                "password-store-remote input is too large",
            )
        } else {
            (self.read_streamed_line)(16 * 1024, "password-store-remote input is too large")
        }
    }

    pub(crate) fn continue_rotation(&self) -> Result<bool> {
        if !(self.stdin_is_terminal)() {
            return Ok(false);
        }
        let answer = (self.read_control_line)("rotate another YubiKey? [y/N]: ")?;
        Ok(is_affirmative(&answer))
    }

    pub(crate) fn write_secret_storage_status(&self, status: &SecretStorageStatus) -> Result<()> {
        for name in status.stored() {
            let name = name.to_string();
            (self.write_line)(&name)?;
        }
        Ok(())
    }

    pub(crate) fn write_ssh_public_key(&self, public_key: &OpenSshPublicKey) -> Result<()> {
        (self.write_line)(public_key.as_str())
    }

    pub(crate) fn confirm_backup_update(
        &self,
        project_name: &str,
        secret_name: &str,
        primary_fingerprint: &str,
        assume_overwrite: bool,
    ) -> Result<bool> {
        if !(self.stdin_is_terminal)() {
            return Ok(assume_overwrite);
        }
        let answer = (self.read_control_line)(&format!(
            "update BWS secret {secret_name} in project {project_name} (primary fingerprint {primary_fingerprint})? [y/N]: "
        ))?;
        Ok(is_affirmative(&answer))
    }

    pub(crate) fn confirm_secret_overwrite(
        &self,
        project_name: &str,
        secret_name: &str,
        assume_overwrite: bool,
    ) -> Result<bool> {
        if !(self.stdin_is_terminal)() {
            return Ok(assume_overwrite);
        }
        let answer = (self.read_control_line)(&format!(
            "update BWS secret {secret_name} in project {project_name}? [y/N]: "
        ))?;
        Ok(is_affirmative(&answer))
    }
}

/// domain summary を固定 JSON schema として stdout へ出す presentation receiver。
///
/// report serialization だけを担い、summary の生成、secret read、外部 state mutation は行わない。
pub(crate) struct JsonReport {
    write_line: LineWriter,
}

impl JsonReport {
    pub(crate) fn new(write_line: LineWriter) -> Self {
        Self { write_line }
    }

    pub(crate) fn write_enroll(&self, summary: &EnrollSummary) -> Result<()> {
        self.write_json(
            &json!({"serial":summary.serial,"role":role(summary.role),"checks":checks(&summary.checks)}),
        )
    }

    pub(crate) fn write_verify(&self, summary: &VerifySummary) -> Result<()> {
        self.write_json(&json!({"serial":summary.serial,"checks":checks(&summary.checks)}))
    }

    pub(crate) fn write_restore_gpg(&self, summary: &RestoreGpgSummary) -> Result<()> {
        self.write_json(
            &json!({"primary_fingerprint":summary.primary_fingerprint,"ssh_key_registered":summary.ssh_key_registered,"ssh_support_ready":summary.ssh_support_ready}),
        )
    }

    pub(crate) fn write_restore_pass(&self, summary: &RestorePassSummary) -> Result<()> {
        self.write_json(
            &json!({"store_path":summary.store_path,"store_readable":summary.store_readable}),
        )
    }

    fn write_json(&self, value: &serde_json::Value) -> Result<()> {
        let line = serde_json::to_string_pretty(value).map_err(anyhow::Error::new)?;
        (self.write_line)(&line)
    }
}

fn is_affirmative(answer: &str) -> bool {
    matches!(answer.trim(), "y" | "Y" | "yes" | "YES" | "Yes")
}

fn checks(checks: &std::collections::BTreeMap<CheckName, CheckStatus>) -> Vec<serde_json::Value> {
    checks
        .iter()
        .map(|(name, status)| json!({"name":name.as_str(),"status":check_status(*status)}))
        .collect()
}

fn role(value: YubikeyRole) -> &'static str {
    match value {
        YubikeyRole::Primary => "primary",
        YubikeyRole::Spare => "spare",
    }
}

fn check_status(value: CheckStatus) -> &'static str {
    match value {
        CheckStatus::Ok => "ok",
        CheckStatus::Failed => "failed",
        CheckStatus::Skipped => "skipped",
    }
}
