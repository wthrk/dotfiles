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
        // session 値は POSIX shell の single-quote エスケープを施して出力し、空白・`;`・`$()`・`#`・`'` 等の
        // 特殊文字を含んでも貼り付け実行で誤解釈されず安全にする（shell 形式整形は presentation 責務）。
        let payload = json!({
            "bw_login": "ok",
            "bw_session": summary.session.as_str(),
        });
        write_json_report(&payload)?;
        eprintln!(
            "export BW_SESSION={}",
            shell_single_quote(summary.session.as_str())
        );
        Ok(())
    }
}

/// 任意の文字列を POSIX shell の single-quote リテラルへエスケープする。
///
/// 値中の各 `'` を `'\''`（quote 閉じ・エスケープした single-quote・quote 開き）へ置換し、
/// 全体を single-quote で囲む。これにより空白・`;`・`$()`・`#`・`'` 等を含む任意の値が、
/// shell へ貼り付けて実行しても元の文字列としてそのまま解釈され、injection を許さない。
/// presentation 形式の整形責務として adapter 層に閉じる（domain には持ち込まない）。
fn shell_single_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
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

#[cfg(test)]
mod tests {
    use super::shell_single_quote;

    #[test]
    fn shell_single_quote_escapes_embedded_single_quote() {
        // `ab'cd` は single-quote を含むため、quote を閉じてエスケープした `'` を挟み再度開く。
        assert_eq!(shell_single_quote("ab'cd"), "'ab'\\''cd'");
    }

    #[test]
    fn shell_single_quote_wraps_plain_value_without_change() {
        // `'` を含まない値（既存 integration の STUBSESSIONKEY== 同様）はそのまま single-quote で囲むだけ。
        assert_eq!(shell_single_quote("STUBSESSIONKEY=="), "'STUBSESSIONKEY=='");
    }

    #[test]
    fn shell_single_quote_preserves_shell_metacharacters() {
        // 空白・`;`・`$()`・`#` などは single-quote 内ではリテラルとして保たれ injection しない。
        assert_eq!(shell_single_quote("a b;$(x)#'"), "'a b;$(x)#'\\'''");
    }
}
