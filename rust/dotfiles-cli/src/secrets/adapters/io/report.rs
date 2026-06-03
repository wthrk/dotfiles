//! verify/enroll summary を CLI 向け JSON report へ変換する adapter。
//!
//! summary の意味づけは domain に残し、この module は出力フォーマットだけを担う。

use serde_json::json;

use crate::{
    Result,
    secrets::{
        domain::{
            bw_login::BwLoginSummary,
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
            "serial": summary.serial,
            "role": report_role(summary.role),
            "checks": report_checks(&summary.checks),
        });
        write_json_report(&payload)
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        let payload = json!({
            "serial": summary.serial,
            "checks": report_checks(&summary.checks),
        });
        write_json_report(&payload)
    }

    fn write_restore_gpg_report(&self, summary: &RestoreGpgSummary) -> Result<()> {
        let payload = json!({
            "primary_fingerprint": summary.primary_fingerprint,
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

    fn write_bw_login_report(&self, summary: &BwLoginSummary) -> Result<()> {
        // `BW_SESSION` を disk / dotfile へ永続化せず、利用者が自分で export できる形で surface する（spec L86）。
        // master password は決して出力しない。session 値は JSON report に含めて stdout に出す。利用者がそのまま
        // 貼れる `export BW_SESSION='...'` 行は stderr に出し、stdout を単一 JSON として機械可読に保つ。
        // session 値は single-quote で囲み、空白・特殊文字を含んでも shell 貼り付けで誤解釈されず安全にする。
        let payload = json!({
            "bw_login": "ok",
            "bw_session": summary.session.as_str(),
        });
        write_json_report(&payload)?;
        eprintln!("export BW_SESSION='{}'", summary.session.as_str());
        Ok(())
    }
}

fn write_json_report(value: &serde_json::Value) -> Result<()> {
    let rendered = serde_json::to_string_pretty(value).map_err(anyhow::Error::new)?;
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
