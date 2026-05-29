//! verify/enroll summary を CLI 向け JSON report へ変換する adapter。
//!
//! summary の意味づけは domain に残し、この module は出力フォーマットだけを担う。

use serde_json::json;

use crate::{
    Result,
    secrets::{
        domain::values::{CheckName, CheckStatus, EnrollSummary, VerifySummary, YubikeyRole},
        ports::ReportPort,
    },
};

#[derive(Default)]
pub(crate) struct JsonReportAdapter;

impl ReportPort for JsonReportAdapter {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()> {
        write_enroll_report(summary)
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        write_verify_report(summary)
    }
}

fn write_enroll_report(summary: &EnrollSummary) -> Result<()> {
    let payload = json!({
        "serial": summary.serial,
        "role": report_role(summary.role),
        "checks": report_checks(&summary.checks),
    });
    let rendered = serde_json::to_string_pretty(&payload).map_err(anyhow::Error::new)?;
    println!("{rendered}");
    Ok(())
}

fn write_verify_report(summary: &VerifySummary) -> Result<()> {
    let payload = json!({
        "serial": summary.serial,
        "checks": report_checks(&summary.checks),
    });
    let rendered = serde_json::to_string_pretty(&payload).map_err(anyhow::Error::new)?;
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
                "name": report_check(*name),
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

fn report_check(value: CheckName) -> &'static str {
    match value {
        CheckName::Setup => "setup",
        CheckName::BwEmail => "bw-email",
        CheckName::BwPassword => "bw-password",
        CheckName::BwsAccessToken => "bws-access-token",
        CheckName::LocalStorage => "local-storage",
        CheckName::Bws => "bws",
        CheckName::BwLogin => "bw-login",
    }
}

fn report_check_status(value: CheckStatus) -> &'static str {
    match value {
        CheckStatus::Ok => "ok",
        CheckStatus::Failed => "failed",
        CheckStatus::Skipped => "skipped",
    }
}
