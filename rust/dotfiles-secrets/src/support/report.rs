//! CLI report JSON formatting support。

use crate::{
    Result,
    domain::{
        enrollment::{EnrollSummary, YubikeyRole},
        gpg_restore::RestoreGpgSummary,
        pass_restore::RestorePassSummary,
        verification::{CheckName, CheckStatus, VerifySummary},
    },
};
use serde_json::json;

pub(crate) fn write_enroll(summary: &EnrollSummary) -> Result<()> {
    write_json(
        &json!({"serial":summary.serial,"role":role(summary.role),"checks":checks(&summary.checks)}),
    )
}
pub(crate) fn write_verify(summary: &VerifySummary) -> Result<()> {
    write_json(&json!({"serial":summary.serial,"checks":checks(&summary.checks)}))
}
pub(crate) fn write_restore_gpg(summary: &RestoreGpgSummary) -> Result<()> {
    write_json(
        &json!({"primary_fingerprint":summary.primary_fingerprint,"ssh_key_registered":summary.ssh_key_registered,"ssh_support_ready":summary.ssh_support_ready}),
    )
}
pub(crate) fn write_restore_pass(summary: &RestorePassSummary) -> Result<()> {
    write_json(&json!({"store_path":summary.store_path,"store_readable":summary.store_readable}))
}
fn write_json(value: &serde_json::Value) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(anyhow::Error::new)?
    );
    Ok(())
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
