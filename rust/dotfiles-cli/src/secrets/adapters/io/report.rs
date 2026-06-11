//! verify/enroll summary を CLI 向け JSON report へ変換する adapter。
//!
//! summary の意味づけは domain に残し、この module は出力フォーマットだけを担う。

use anyhow::Context;
use serde_json::json;

use crate::{
    Result,
    secrets::{
        domain::{
            enrollment::{EnrollSummary, YubikeyRole},
            gpg_restore::RestoreGpgSummary,
            pass_restore::RestorePassSummary,
            verification::{CheckName, CheckStatus, VerifySummary},
        },
        ports::io::ReportPort,
    },
};

/// verify/enroll summary を `ReportPort` の CLI JSON 出力へ翻訳する adapter。
#[derive(Default)]
pub(super) struct JsonReportAdapter;

impl ReportPort for JsonReportAdapter {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()> {
        let payload = json!({
            "role": report_role(summary.role),
            "checks": report_checks(&summary.checks),
        });
        write_json_report(&payload)
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        let payload = json!({
            "checks": report_checks(&summary.checks),
        });
        write_json_report(&payload)
    }

    fn write_restore_gpg_report(&self, summary: &RestoreGpgSummary) -> Result<()> {
        let payload = json!({
            "ssh_key_registered": summary.ssh_key_registered,
            "ssh_support_ready": summary.ssh_support_ready,
        });
        write_json_report(&payload)
    }

    fn write_restore_pass_report(&self, summary: &RestorePassSummary) -> Result<()> {
        let payload = json!({
            "store_path": summary.store_path,
            "store_readable": summary.store_readable,
        });
        write_json_report(&payload)
    }
}

fn write_json_report(value: &serde_json::Value) -> Result<()> {
    let rendered =
        serde_json::to_string_pretty(value).context("failed to render CLI JSON report")?;
    println!("{rendered}");
    Ok(())
}

fn report_checks(
    checks: &std::collections::BTreeMap<CheckName, CheckStatus>,
) -> Vec<serde_json::Value> {
    checks
        .iter()
        .map(|(name, status)| {
            json!({
                "name": name.as_str(),
                "status": report_check_status(*status),
            })
        })
        .collect()
}

fn report_role(value: YubikeyRole) -> &'static str {
    match value {
        YubikeyRole::Primary => "primary",
        YubikeyRole::Spare => "spare",
    }
}

fn report_check_status(value: CheckStatus) -> &'static str {
    match value {
        CheckStatus::Ok => "ok",
        CheckStatus::Failed => "failed",
        CheckStatus::Skipped => "skipped",
    }
}
