//! report port 契約を JSON/stdout 出力へ翻訳する adapter。

use anyhow::Context;
use serde_json::json;

use crate::{
    Result,
    secrets::domain::values::{CheckName, CheckStatus, EnrollSummary, VerifySummary, YubikeyRole},
    secrets::ports::ReportPort,
};

/// application が渡す report 列挙値を CLI 向け JSON payload に整形する runtime adapter。
pub(super) struct JsonReportAdapter;

impl JsonReportAdapter {
    /// enroll 結果を route 監査情報つき JSON report へ翻訳して stdout へ出力する。
    ///
    /// この関数は adapter 翻訳境界として、domain/application 値を CLI 出力契約へ
    /// 変換する責務のみを持つ。caller 側は route 判定済みの境界値を渡し、
    /// ここで route 判定ロジックを追加しない責務を負う。
    pub(super) fn write_enroll_report_for_route(
        &self,
        summary: &EnrollSummary,
        route: &'static str,
    ) -> Result<()> {
        let payload = json!({
            "serial": summary.serial,
            "role": Self::report_role(summary.role),
            "checks": Self::report_checks(&summary.checks),
            "device-adapter-route": route,
        });
        let rendered =
            serde_json::to_string_pretty(&payload).context("failed to serialize report")?;
        println!("{rendered}");
        Ok(())
    }

    /// verify 結果を route 監査情報つき JSON report へ翻訳して stdout へ出力する。
    ///
    /// adapter では「report 形式への写像」と「出力」だけを扱い、route 選択は扱わない。
    /// caller 側は same-route 監査で確定した route 値を渡し、境界外で別ルートを
    /// 生成しないことが責務となる。
    pub(super) fn write_verify_report_for_route(
        &self,
        summary: &VerifySummary,
        route: &'static str,
    ) -> Result<()> {
        let payload = json!({
            "serial": summary.serial,
            "checks": Self::report_checks(&summary.checks),
            "device-adapter-route": route,
        });
        let rendered =
            serde_json::to_string_pretty(&payload).context("failed to serialize report")?;
        println!("{rendered}");
        Ok(())
    }
}

impl ReportPort for JsonReportAdapter {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()> {
        self.write_enroll_report_for_route(summary, "real")
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        self.write_verify_report_for_route(summary, "real")
    }
}

impl JsonReportAdapter {
    /// domain 側の check map を CLI JSON 配列形式へ翻訳する。
    ///
    /// check 名と状態の表記は外部出力契約なので、domain 値の意味を変えずにここで文字列化する。
    fn report_checks(
        checks: &std::collections::BTreeMap<CheckName, CheckStatus>,
    ) -> Vec<serde_json::Value> {
        checks
            .iter()
            .map(|(name, status)| {
                json!({
                    "name": Self::report_check(*name),
                    "status": Self::report_check_status(*status),
                })
            })
            .collect()
    }

    /// domain role 列挙値を JSON wire の安定 key 文字列へ写像する。
    fn report_role(value: YubikeyRole) -> &'static str {
        match value {
            YubikeyRole::Primary => "primary",
            YubikeyRole::Spare => "spare",
        }
    }

    /// domain check 名を互換性維持対象の JSON key 文字列へ翻訳する。
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

    /// domain status 列挙値を report wire status 文字列へ翻訳する。
    fn report_check_status(value: CheckStatus) -> &'static str {
        match value {
            CheckStatus::Ok => "ok",
            CheckStatus::Failed => "failed",
            CheckStatus::Skipped => "skipped",
        }
    }
}
