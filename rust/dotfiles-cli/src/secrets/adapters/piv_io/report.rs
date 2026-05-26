//! report port 契約を JSON/stdout 出力へ翻訳する adapter。

use anyhow::Context;
use serde_json::json;

use crate::{
    Result,
    secrets::domain::{CheckName, CheckStatus, EnrollSummary, VerifySummary, YubikeyRole},
    secrets::ports::ReportPort,
};

/// application が渡す report 列挙値を CLI 向け JSON payload に整形する runtime adapter。
pub(super) struct JsonReportAdapter;

impl ReportPort for JsonReportAdapter {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()> {
        let payload = json!({
            "serial": summary.serial,
            "role": report_role(summary.role),
            "checks": report_checks(&summary.checks),
        });
        let rendered =
            serde_json::to_string_pretty(&payload).context("failed to serialize report")?;
        println!("{rendered}");
        Ok(())
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        let payload = json!({
            "serial": summary.serial,
            "checks": report_checks(&summary.checks),
        });
        let rendered =
            serde_json::to_string_pretty(&payload).context("failed to serialize report")?;
        println!("{rendered}");
        Ok(())
    }

    fn report_primary_enrollment(&self, serial: u32) -> Result<()> {
        self.write_enroll_report(&EnrollSummary::primary_completed(serial))
    }

    fn report_spare_enrollment(&self, serial: u32) -> Result<()> {
        self.write_enroll_report(&EnrollSummary::spare_completed(serial))
    }

    fn report_local_storage_verified(&self, serial: u32) -> Result<()> {
        self.write_verify_report(&VerifySummary::local_storage_verified(serial))
    }

    fn report_local_storage_failed(&self, serial: u32) -> Result<()> {
        self.write_verify_report(&VerifySummary::local_storage_failed(serial))
    }

    fn report_external_checks_unavailable(
        &self,
        serial: u32,
        checks: impl IntoIterator<Item = CheckName>,
    ) -> Result<()> {
        self.write_verify_report(&VerifySummary::external_checks_unavailable(serial, checks))
    }
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
