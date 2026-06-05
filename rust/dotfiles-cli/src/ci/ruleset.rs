//! GitHub に実適用された ruleset が版管理された安全要件を満たすかを機械判定する純粋規則。
//!
//! `.github/rulesets/nightly-bump.json` は版管理されるが、GitHub への適用は手動 `gh api` 依存である。
//! 適用漏れ・bypass actor の後付け・required check の context ドリフトが起きると、required status check
//! `nightly-bump-guard` が無効化されて nightly 無人 auto-merge 経路が fail-open する。この module は CI が
//! `gh api repos/{owner}/{repo}/rulesets/{id}` で取得した「実際に適用されている ruleset」の JSON を受け取り、
//! 次の不変条件を満たすかを I/O 無しで判定する。
//!
//! 1. **enforcement=active**: ruleset が無効化（`disabled` / `evaluate`）されていないこと。
//! 2. **bypass_actors 空**: admin / App を含めいかなる actor も bypass できないこと（required check の
//!    fail-open を塞ぐ）。
//! 3. **required status check 包含**: required_status_checks ルールに guard の context（`nightly-bump-guard`）
//!    が含まれること。これが落ちると guard が required でなくなり fail-open する。
//!
//! いずれかに違反すれば [`verify_applied_ruleset`] は違反理由を載せた `Err` を返し、CLI は非 0 終了する。
//! 適用状態の取得（`gh api`）は CLI 側の責務で、本 module は取得済み JSON の判定だけを担う。

use anyhow::{Context, bail};
use serde_json::Value;

use crate::Result;

/// guard が required であることを保証するために存在しなければならない status check context。
///
/// `.github/workflows/nightly-bump-guard.yml` の job 名・`.github/rulesets/nightly-bump.json` の
/// required check context と一致する load-bearing な固定値。ここがドリフトすると required check が
/// 無効化され fail-open するため、CI が継続検証する。
pub(crate) const REQUIRED_GUARD_CONTEXT: &str = "nightly-bump-guard";

/// GitHub に適用された単一 ruleset の JSON が安全要件を満たすかを判定する。
///
/// `applied` は `gh api repos/{owner}/{repo}/rulesets/{id}` の応答（単一 ruleset。`rules` を含む詳細表現）。
/// enforcement・bypass_actors・required status check の 3 不変条件を順に検査し、最初の違反理由を載せた
/// `Err` を返す。すべて満たせば `Ok(())`。
///
/// caller responsibility: `applied` は ruleset 一覧（`/rulesets`）の要素ではなく、`rules` 配列を含む
/// 詳細表現（`/rulesets/{id}`）であること。一覧表現は `rules` を持たないため required check を検査できない。
pub(crate) fn verify_applied_ruleset(applied: &str) -> Result<()> {
    let ruleset: Value =
        serde_json::from_str(applied).context("applied ruleset response is not valid JSON")?;

    verify_enforcement(&ruleset)?;
    verify_bypass_actors_empty(&ruleset)?;
    verify_required_guard_check(&ruleset)?;
    Ok(())
}

/// `enforcement` が `active` であることを検査する。
fn verify_enforcement(ruleset: &Value) -> Result<()> {
    match ruleset.get("enforcement").and_then(Value::as_str) {
        Some("active") => Ok(()),
        other => bail!(
            "applied ruleset enforcement is {other:?}, expected \"active\"; \
             required check is not enforced (fail-open)"
        ),
    }
}

/// `bypass_actors` が空（または欠落）であることを検査する。
///
/// admin / App / role いずれの bypass actor も許さない。1 つでも存在すれば required check を回避して
/// 無人 merge できる経路が開くため fail とする。
fn verify_bypass_actors_empty(ruleset: &Value) -> Result<()> {
    match ruleset.get("bypass_actors") {
        // 欠落 / null は bypass 無しとみなす。
        None | Some(Value::Null) => Ok(()),
        Some(Value::Array(actors)) if actors.is_empty() => Ok(()),
        Some(Value::Array(actors)) => bail!(
            "applied ruleset has {} bypass actor(s); must be empty so admin/App cannot \
             bypass the required check",
            actors.len()
        ),
        Some(other) => bail!("applied ruleset bypass_actors is not an array: {other}"),
    }
}

/// required_status_checks ルールに guard の context が含まれることを検査する。
///
/// `rules` 配列から `type == "required_status_checks"` のルールを探し、その
/// `parameters.required_status_checks[].context` に [`REQUIRED_GUARD_CONTEXT`] があることを確認する。
/// ルール不在・context 不在はいずれも fail（guard が required でなくなる）。
fn verify_required_guard_check(ruleset: &Value) -> Result<()> {
    let rules = ruleset
        .get("rules")
        .and_then(Value::as_array)
        .context("applied ruleset has no rules array (need /rulesets/{id} detail, not list)")?;

    let required_rule = rules
        .iter()
        .find(|rule| rule.get("type").and_then(Value::as_str) == Some("required_status_checks"));
    let Some(required_rule) = required_rule else {
        bail!(
            "applied ruleset has no required_status_checks rule; guard is not required (fail-open)"
        );
    };

    let contexts = required_rule
        .get("parameters")
        .and_then(|parameters| parameters.get("required_status_checks"))
        .and_then(Value::as_array)
        .context("required_status_checks rule has no required_status_checks parameter array")?;

    let has_guard = contexts
        .iter()
        .filter_map(|check| check.get("context").and_then(Value::as_str))
        .any(|context| context == REQUIRED_GUARD_CONTEXT);

    if has_guard {
        Ok(())
    } else {
        bail!(
            "applied ruleset required checks do not include `{REQUIRED_GUARD_CONTEXT}`; \
             guard context drifted and is no longer required (fail-open)"
        );
    }
}

#[cfg(test)]
mod tests {
    //! 適用済み ruleset JSON の 3 不変条件（active / bypass 空 / guard context 包含）と、
    //! それぞれが破れたとき fail することを固定する。

    use super::*;

    /// 全不変条件を満たす最小の適用済み ruleset 詳細 JSON。
    fn valid_ruleset() -> String {
        r#"{
          "name": "nightly-bump-protection",
          "enforcement": "active",
          "bypass_actors": [],
          "rules": [
            { "type": "deletion" },
            {
              "type": "required_status_checks",
              "parameters": {
                "required_status_checks": [
                  { "context": "nightly-bump-guard" }
                ]
              }
            }
          ]
        }"#
        .to_string()
    }

    #[test]
    fn accepts_valid_applied_ruleset() -> Result<()> {
        verify_applied_ruleset(&valid_ruleset())
    }

    #[test]
    fn rejects_inactive_enforcement() {
        let applied = valid_ruleset().replace(r#""active""#, r#""evaluate""#);
        let err = verify_applied_ruleset(&applied).unwrap_err();
        assert!(err.to_string().contains("enforcement"), "{err}");
    }

    #[test]
    fn rejects_non_empty_bypass_actors() {
        let applied = valid_ruleset().replace(
            r#""bypass_actors": [],"#,
            r#""bypass_actors": [ { "actor_id": 1, "actor_type": "OrganizationAdmin", "bypass_mode": "always" } ],"#,
        );
        let err = verify_applied_ruleset(&applied).unwrap_err();
        assert!(err.to_string().contains("bypass actor"), "{err}");
    }

    #[test]
    fn rejects_missing_required_status_checks_rule() {
        let applied = r#"{
          "enforcement": "active",
          "bypass_actors": [],
          "rules": [ { "type": "deletion" } ]
        }"#;
        let err = verify_applied_ruleset(applied).unwrap_err();
        assert!(
            err.to_string().contains("no required_status_checks rule"),
            "{err}"
        );
    }

    #[test]
    fn rejects_guard_context_drift() {
        let applied = valid_ruleset().replace("nightly-bump-guard", "some-other-check");
        let err = verify_applied_ruleset(&applied).unwrap_err();
        assert!(err.to_string().contains("do not include"), "{err}");
    }

    #[test]
    fn rejects_list_representation_without_rules() {
        // /rulesets 一覧表現（rules 無し）は required check を検査できないため fail。
        let applied = r#"{ "name": "x", "enforcement": "active", "bypass_actors": [] }"#;
        let err = verify_applied_ruleset(applied).unwrap_err();
        assert!(err.to_string().contains("no rules array"), "{err}");
    }
}
