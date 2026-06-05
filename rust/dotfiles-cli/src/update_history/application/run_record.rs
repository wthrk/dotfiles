//! record use case: version 差分を集め、各アプリのリリースノートを LLM で構造化抽出して履歴へ追記する。

use crate::Result;
use crate::update_history::domain::build::{PackageMaterial, build_entry};
use crate::update_history::domain::commands::RecordCommand;
use crate::update_history::domain::diff::merge_version_deltas;
use crate::update_history::domain::validate::{sanitize_change_items, sanitize_notes_url};
use crate::update_history::ports::{
    BrewVersionDiffPort, ChangeExtractPort, ClosureDiffPort, HistoryStorePort, NotesPort,
};

/// nix/brew の version 差分を統合し、各アプリの生ノートを LLM 抽出して 1 エントリを履歴へ追記する。
///
/// 順序制御の理由: 差分（nix→brew）を先に確定してから各アプリのノート取得・LLM 抽出を行うのは、
/// 差分に現れたアプリだけをノート取得・抽出対象にし、無関係なノート取得を避けるためである。停止条件は
/// 各 port の `Err` 伝播であり、ノート取得不能（`None`）や LLM 未使用（空配列）はフォールバックとして
/// version + URL のみへ縮退させ、record 全体は失敗させない。LLM 出力と生ノート URL は信頼境界外のため、
/// 記録前に必ず domain の機械バリデート（host allowlist / 長さ / 件数）を通す。severity / overall の算出は
/// domain（[`build_entry`]）に委ね、application は素材収集の順序だけを保持する。
pub(crate) fn run_record<D, B, N, X, S>(
    command: RecordCommand,
    closure_diff: &D,
    brew_diff: &B,
    notes: &N,
    extract: &X,
    store: &S,
) -> Result<()>
where
    D: ClosureDiffPort,
    B: BrewVersionDiffPort,
    N: NotesPort,
    X: ChangeExtractPort,
    S: HistoryStorePort,
{
    let nix_deltas = closure_diff.diff_closures(&command.old_closure, &command.new_closure)?;
    let brew_deltas = brew_diff.diff_brew_versions(&command.old_rev, &command.new_rev)?;
    let deltas = merge_version_deltas(nix_deltas, brew_deltas);

    let mut materials = Vec::with_capacity(deltas.len());
    for delta in deltas {
        // 各アプリの `(old, new]` 生ノートを取得し、取得できたものだけ LLM 抽出へ回す。
        let raw = notes.fetch_release_notes(&delta.name, delta.old.clone(), delta.new.clone())?;
        let (change_items, notes_url) = match raw {
            Some(raw) => {
                // LLM 出力は信頼境界外。記録前に host/長さ/件数を機械バリデートする。
                let extracted = extract.extract_change_items(&raw)?;
                (
                    sanitize_change_items(extracted),
                    sanitize_notes_url(Some(raw.notes_url)),
                )
            }
            // ノート取得不能なら version 差分のみ（変更リスト空・URL なし）へ縮退する。
            None => (Vec::new(), None),
        };
        materials.push(PackageMaterial {
            delta,
            change_items,
            notes_url,
        });
    }

    let entry = build_entry(
        command.at,
        command.nixpkgs_old,
        command.nixpkgs_new,
        command.reference,
        materials,
    );
    store.append_entry(&entry)
}

#[cfg(test)]
mod tests {
    //! record の順序（nix/brew diff → ノート取得 → LLM 抽出 → サニタイズ → 追記）と
    //! フォールバック（ノート不在で version のみ）・バリデート（不正 URL 破棄）を mockall mock で固定する。

    use super::run_record;
    use crate::update_history::domain::commands::RecordCommand;
    use crate::update_history::domain::diff::{DeltaSource, VersionDelta};
    use crate::update_history::domain::wire::{ChangeCategory, ChangeItem, ChangeKind, Severity};
    use crate::update_history::ports::{
        MockBrewVersionDiffPort, MockChangeExtractPort, MockClosureDiffPort, MockHistoryStorePort,
        MockNotesPort, RawReleaseNotes,
    };

    fn command() -> RecordCommand {
        RecordCommand {
            old_closure: "/nix/old".to_string(),
            new_closure: "/nix/new".to_string(),
            old_rev: "oldrev".to_string(),
            new_rev: "newrev".to_string(),
            nixpkgs_old: "a1b2c3d".to_string(),
            nixpkgs_new: "e4f5g6h".to_string(),
            reference: "darwinConfigurations.ci".to_string(),
            at: "2026-06-05T18:00:11Z".to_string(),
        }
    }

    fn nix_delta(name: &str) -> VersionDelta {
        VersionDelta {
            name: name.to_string(),
            old: Some("1.0".to_string()),
            new: Some("1.1".to_string()),
            change: ChangeKind::Upgraded,
            source: DeltaSource::NixClosure,
        }
    }

    #[test]
    fn record_extracts_sanitizes_and_appends_one_entry() -> crate::Result<()> {
        let mut closure_diff = MockClosureDiffPort::new();
        closure_diff
            .expect_diff_closures()
            .times(1)
            .returning(|_, _| Ok(vec![nix_delta("openssl")]));
        let mut brew_diff = MockBrewVersionDiffPort::new();
        brew_diff
            .expect_diff_brew_versions()
            .times(1)
            .returning(|_, _| Ok(Vec::new()));

        let mut notes = MockNotesPort::new();
        notes
            .expect_fetch_release_notes()
            .times(1)
            .returning(|_, _, _| {
                Ok(Some(RawReleaseNotes {
                    text: "CVE fix".to_string(),
                    notes_url: "https://github.com/openssl/openssl/releases/tag/v1.1".to_string(),
                }))
            });

        let mut extract = MockChangeExtractPort::new();
        extract
            .expect_extract_change_items()
            .times(1)
            .returning(|_| {
                Ok(vec![
                    ChangeItem {
                        category: ChangeCategory::Security,
                        text: "CVE 修正".to_string(),
                        // 許可外 host の ref は記録前に破棄される。
                        ref_url: Some("https://evil.example/x".to_string()),
                    },
                    ChangeItem {
                        category: ChangeCategory::Feature,
                        text: "新機能".to_string(),
                        ref_url: Some("https://github.com/openssl/openssl/pull/2".to_string()),
                    },
                ])
            });

        let mut store = MockHistoryStorePort::new();
        store
            .expect_append_entry()
            .times(1)
            .withf(|entry| {
                entry.at == "2026-06-05T18:00:11Z"
                    && entry.severity == Severity::Critical
                    && entry.overall == "1アプリ更新: 🔒1 ✨1"
                    && entry.packages.len() == 1
                    && entry.packages[0].name == "openssl"
                    && entry.packages[0].change_items.len() == 2
                    // 許可外 host の ref は None、許可 host の ref は保持。
                    && entry.packages[0].change_items[0].ref_url.is_none()
                    && entry.packages[0].change_items[1].ref_url.as_deref()
                        == Some("https://github.com/openssl/openssl/pull/2")
                    && entry.packages[0].notes_url.as_deref()
                        == Some("https://github.com/openssl/openssl/releases/tag/v1.1")
            })
            .returning(|_| Ok(()));

        run_record(
            command(),
            &closure_diff,
            &brew_diff,
            &notes,
            &extract,
            &store,
        )
    }

    #[test]
    fn record_falls_back_to_version_only_when_notes_absent() -> crate::Result<()> {
        let mut closure_diff = MockClosureDiffPort::new();
        closure_diff
            .expect_diff_closures()
            .returning(|_, _| Ok(vec![nix_delta("neovim")]));
        let mut brew_diff = MockBrewVersionDiffPort::new();
        brew_diff
            .expect_diff_brew_versions()
            .returning(|_, _| Ok(Vec::new()));

        let mut notes = MockNotesPort::new();
        // ノート取得不能なら LLM 抽出は呼ばれない。
        notes
            .expect_fetch_release_notes()
            .times(1)
            .returning(|_, _, _| Ok(None));
        let mut extract = MockChangeExtractPort::new();
        extract.expect_extract_change_items().never();

        let mut store = MockHistoryStorePort::new();
        store
            .expect_append_entry()
            .times(1)
            .withf(|entry| {
                entry.severity == Severity::None
                    && entry.packages.len() == 1
                    && entry.packages[0].change_items.is_empty()
                    && entry.packages[0].notes_url.is_none()
            })
            .returning(|_| Ok(()));

        run_record(
            command(),
            &closure_diff,
            &brew_diff,
            &notes,
            &extract,
            &store,
        )
    }
}
