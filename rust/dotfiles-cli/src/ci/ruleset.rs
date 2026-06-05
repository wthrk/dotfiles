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
//! 3. **required status check 包含と strict 一致**: required_status_checks ルールに guard の context
//!    （`nightly-bump-guard`）が含まれること。これが落ちると guard が required でなくなり fail-open する。
//!    加えて、適用済み ruleset の `strict_required_status_checks_policy`（merge 前に base へ最新化を要求する
//!    strict フラグ）が版管理 JSON（`.github/rulesets/nightly-bump.json`）の要求値と一致すること。版管理が
//!    strict を要求しているのに適用側で false に drift すると、古い base のまま auto-merge されうるため検証する。
//! 4. **適用対象が default branch を含む**: `target == "branch"` かつ `conditions.ref_name.include` が
//!    `~DEFAULT_BRANCH`（default branch を表す GitHub のメタ ref）を含むこと。retarget（default branch を含まない
//!    別 ref へ条件を差し替え）されると、ruleset 自体は active / bypass 空 / required check ありでも main を保護
//!    せず、nightly 無人 auto-merge が無保護の main へ流れる（実質 fail-open）。対象条件まで検証して塞ぐ。
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

/// default branch を表す GitHub ruleset の組込みメタ ref。
///
/// `conditions.ref_name.include` がこれを含むと、ruleset は repository の default branch（main）へ適用される。
/// retarget でこれが落ちると main が保護されなくなるため、適用対象検証の load-bearing な固定値。
const DEFAULT_BRANCH_REF: &str = "~DEFAULT_BRANCH";

/// GitHub に適用された単一 ruleset の JSON が安全要件を満たすかを判定する。
///
/// `applied` は `gh api repos/{owner}/{repo}/rulesets/{id}` の応答（単一 ruleset。`rules` を含む詳細表現）。
/// `definition` は版管理された ruleset 定義 JSON（`.github/rulesets/nightly-bump.json`）で、strict policy 等の
/// 要求値の source of truth として使う。enforcement・bypass_actors・適用対象・required status check（context
/// 包含と strict policy 一致）の不変条件を順に検査し、最初の違反理由を載せた `Err` を返す。すべて満たせば `Ok(())`。
///
/// caller responsibility: `applied` は ruleset 一覧（`/rulesets`）の要素ではなく、`rules` 配列を含む
/// 詳細表現（`/rulesets/{id}`）であること。一覧表現は `rules` を持たないため required check を検査できない。
pub(crate) fn verify_applied_ruleset(applied: &str, definition: &str) -> Result<()> {
    let ruleset: Value =
        serde_json::from_str(applied).context("applied ruleset response is not valid JSON")?;
    let definition: Value = serde_json::from_str(definition)
        .context("versioned ruleset definition is not valid JSON")?;

    verify_enforcement(&ruleset)?;
    verify_bypass_actors_empty(&ruleset)?;
    verify_applied_target(&ruleset)?;
    verify_required_guard_check(&ruleset)?;
    verify_strict_policy_matches(&ruleset, &definition)?;
    Ok(())
}

/// 適用済み ruleset の `strict_required_status_checks_policy` が版管理 JSON の要求値に一致するかを検査する。
///
/// 版管理 JSON（`definition`）の required_status_checks rule が持つ `strict_required_status_checks_policy` を
/// 期待値とし、適用済み ruleset（`applied`）の同フィールドがそれと一致することを要求する。版管理が strict を
/// 要求しているのに適用側で false へ drift していると、古い base のまま auto-merge されうる（merge 前最新化の
/// 強制が外れる）ため fail にする。両者とも欠落時の既定は GitHub の既定どおり `false` とみなして比較する
/// （版管理が strict を明示要求していなければ適用側も false で整合）。
fn verify_strict_policy_matches(applied: &Value, definition: &Value) -> Result<()> {
    let expected = strict_policy(definition);
    let actual = strict_policy(applied);
    if expected == actual {
        Ok(())
    } else {
        bail!(
            "applied ruleset strict_required_status_checks_policy is {actual}, \
             but versioned definition requires {expected}; strict policy drifted \
             (auto-merge may bypass base up-to-date requirement)"
        )
    }
}

/// ruleset 値から required_status_checks rule の `strict_required_status_checks_policy` を読む（欠落は `false`）。
///
/// 版管理 JSON と適用済み JSON の双方で同じ抽出規則を共有する。required_status_checks rule が無い、または
/// strict フラグが無い場合は GitHub 既定の `false` とみなす。
fn strict_policy(ruleset: &Value) -> bool {
    ruleset
        .get("rules")
        .and_then(Value::as_array)
        .and_then(|rules| {
            rules.iter().find(|rule| {
                rule.get("type").and_then(Value::as_str) == Some("required_status_checks")
            })
        })
        .and_then(|rule| rule.get("parameters"))
        .and_then(|parameters| parameters.get("strict_required_status_checks_policy"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// 適用対象が default branch（main）を含むことを検査する（retarget による無保護化を塞ぐ）。
///
/// `target` が `branch` であり、`conditions.ref_name.include` に [`DEFAULT_BRANCH_REF`] が含まれることを
/// 確認する。`target` が branch 以外（tag / push 等）へ差し替えられた、または include から default branch が
/// 落とされた retarget は、ruleset が他の不変条件（active / bypass 空 / required check）を満たしていても
/// main を保護しないため fail とする。`exclude` で default branch が除外されていれば、include にあっても
/// 保護が外れるため fail とする。
fn verify_applied_target(ruleset: &Value) -> Result<()> {
    match ruleset.get("target").and_then(Value::as_str) {
        Some("branch") => {}
        other => bail!(
            "applied ruleset target is {other:?}, expected \"branch\"; \
             retargeted ruleset does not protect the default branch (fail-open)"
        ),
    }

    let ref_name = ruleset
        .get("conditions")
        .and_then(|conditions| conditions.get("ref_name"))
        .context(
            "applied ruleset has no conditions.ref_name; cannot confirm it targets the \
             default branch (fail-open)",
        )?;

    if ref_name_array_contains(ref_name, "exclude", DEFAULT_BRANCH_REF) {
        bail!(
            "applied ruleset excludes `{DEFAULT_BRANCH_REF}` in conditions.ref_name.exclude; \
             the default branch is not protected (fail-open)"
        );
    }

    if ref_name_array_contains(ref_name, "include", DEFAULT_BRANCH_REF) {
        Ok(())
    } else {
        bail!(
            "applied ruleset conditions.ref_name.include does not contain `{DEFAULT_BRANCH_REF}`; \
             retargeted away from the default branch (fail-open)"
        )
    }
}

/// `ref_name.<field>`（include / exclude）配列に指定 ref が含まれるかを判定する。
///
/// 配列が無い / 文字列でない要素は無視する。include の包含判定と exclude の除外判定の双方で共有する。
fn ref_name_array_contains(ref_name: &Value, field: &str, target_ref: &str) -> bool {
    ref_name
        .get(field)
        .and_then(Value::as_array)
        .is_some_and(|refs| {
            refs.iter()
                .filter_map(Value::as_str)
                .any(|value| value == target_ref)
        })
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
    //! 適用済み ruleset JSON の不変条件（active / bypass 空 / 適用対象 / guard context 包含 / strict policy
    //! 一致）と、それぞれが破れたとき fail することを固定する。

    use super::*;

    /// 版管理 ruleset 定義 JSON（strict policy を true で要求する `.github/rulesets/nightly-bump.json` 相当）。
    fn definition() -> String {
        r#"{
          "name": "nightly-bump-protection",
          "rules": [
            {
              "type": "required_status_checks",
              "parameters": {
                "strict_required_status_checks_policy": true,
                "required_status_checks": [ { "context": "nightly-bump-guard" } ]
              }
            }
          ]
        }"#
        .to_string()
    }

    /// 全不変条件を満たす最小の適用済み ruleset 詳細 JSON（strict policy も版管理要求に一致させる）。
    fn valid_ruleset() -> String {
        r#"{
          "name": "nightly-bump-protection",
          "target": "branch",
          "enforcement": "active",
          "bypass_actors": [],
          "conditions": {
            "ref_name": { "include": ["~DEFAULT_BRANCH"], "exclude": [] }
          },
          "rules": [
            { "type": "deletion" },
            {
              "type": "required_status_checks",
              "parameters": {
                "strict_required_status_checks_policy": true,
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
        verify_applied_ruleset(&valid_ruleset(), &definition())
    }

    #[test]
    fn rejects_inactive_enforcement() {
        let applied = valid_ruleset().replace(r#""active""#, r#""evaluate""#);
        let err = verify_applied_ruleset(&applied, &definition()).unwrap_err();
        assert!(err.to_string().contains("enforcement"), "{err}");
    }

    #[test]
    fn rejects_non_empty_bypass_actors() {
        let applied = valid_ruleset().replace(
            r#""bypass_actors": [],"#,
            r#""bypass_actors": [ { "actor_id": 1, "actor_type": "OrganizationAdmin", "bypass_mode": "always" } ],"#,
        );
        let err = verify_applied_ruleset(&applied, &definition()).unwrap_err();
        assert!(err.to_string().contains("bypass actor"), "{err}");
    }

    #[test]
    fn rejects_missing_required_status_checks_rule() {
        let applied = r#"{
          "target": "branch",
          "enforcement": "active",
          "bypass_actors": [],
          "conditions": { "ref_name": { "include": ["~DEFAULT_BRANCH"], "exclude": [] } },
          "rules": [ { "type": "deletion" } ]
        }"#;
        let err = verify_applied_ruleset(applied, &definition()).unwrap_err();
        assert!(
            err.to_string().contains("no required_status_checks rule"),
            "{err}"
        );
    }

    #[test]
    fn rejects_target_retargeted_away_from_branch() {
        // P2-5 退行固定: target が branch 以外（tag 等）へ差し替えられた ruleset は main を保護しないため fail。
        let applied = valid_ruleset().replace(r#""target": "branch""#, r#""target": "tag""#);
        let err = verify_applied_ruleset(&applied, &definition()).unwrap_err();
        assert!(err.to_string().contains("target is"), "{err}");
    }

    #[test]
    fn rejects_conditions_without_default_branch_include() {
        // P2-5 退行固定: conditions.ref_name.include が `~DEFAULT_BRANCH` を含まない（別 ref へ retarget）と、
        // active / bypass 空 / required check ありでも main を保護しないため fail。
        let applied = valid_ruleset().replace(
            r#""include": ["~DEFAULT_BRANCH"]"#,
            r#""include": ["refs/heads/some-other-branch"]"#,
        );
        let err = verify_applied_ruleset(&applied, &definition()).unwrap_err();
        assert!(err.to_string().contains("does not contain"), "{err}");
    }

    #[test]
    fn rejects_default_branch_excluded() {
        // include に `~DEFAULT_BRANCH` があっても exclude で除外されれば保護が外れるため fail。
        let applied =
            valid_ruleset().replace(r#""exclude": []"#, r#""exclude": ["~DEFAULT_BRANCH"]"#);
        let err = verify_applied_ruleset(&applied, &definition()).unwrap_err();
        assert!(err.to_string().contains("excludes"), "{err}");
    }

    #[test]
    fn rejects_missing_conditions() {
        // conditions.ref_name 欠落は default branch 適用を確認できないため fail。
        let applied = r#"{
          "target": "branch",
          "enforcement": "active",
          "bypass_actors": [],
          "rules": [ { "type": "deletion" } ]
        }"#;
        let err = verify_applied_ruleset(applied, &definition()).unwrap_err();
        assert!(err.to_string().contains("no conditions.ref_name"), "{err}");
    }

    #[test]
    fn rejects_guard_context_drift() {
        let applied = valid_ruleset().replace("nightly-bump-guard", "some-other-check");
        let err = verify_applied_ruleset(&applied, &definition()).unwrap_err();
        assert!(err.to_string().contains("do not include"), "{err}");
    }

    #[test]
    fn rejects_strict_policy_drift() {
        // N10 退行固定: 版管理 JSON が strict policy を true で要求するのに、適用済み ruleset で false へ
        // drift していると、古い base のまま auto-merge されうるため fail。
        let applied = valid_ruleset().replace(
            r#""strict_required_status_checks_policy": true"#,
            r#""strict_required_status_checks_policy": false"#,
        );
        let err = verify_applied_ruleset(&applied, &definition()).unwrap_err();
        assert!(
            err.to_string()
                .contains("strict_required_status_checks_policy"),
            "{err}"
        );
    }

    #[test]
    fn rejects_strict_policy_absent_when_required() {
        // 適用済み ruleset が strict フラグを持たない（既定 false）のに版管理が true 要求なら fail。
        let applied =
            valid_ruleset().replace(r#""strict_required_status_checks_policy": true,"#, "");
        let err = verify_applied_ruleset(&applied, &definition()).unwrap_err();
        assert!(
            err.to_string()
                .contains("strict_required_status_checks_policy"),
            "{err}"
        );
    }

    #[test]
    fn rejects_list_representation_without_rules() {
        // /rulesets 一覧表現（rules 無し）は required check を検査できないため fail。
        let applied = r#"{
          "name": "x",
          "target": "branch",
          "enforcement": "active",
          "bypass_actors": [],
          "conditions": { "ref_name": { "include": ["~DEFAULT_BRANCH"], "exclude": [] } }
        }"#;
        let err = verify_applied_ruleset(applied, &definition()).unwrap_err();
        assert!(err.to_string().contains("no rules array"), "{err}");
    }
}
