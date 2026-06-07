//! record use case: version 差分を集め、各アプリのリリースノートを LLM で構造化抽出して履歴へ追記する。

use crate::Result;
use crate::update_history::domain::build::{PackageMaterial, build_entry};
use crate::update_history::domain::commands::RecordCommand;
use crate::update_history::domain::diff::{diff_versions, merge_version_deltas};
use crate::update_history::domain::validate::{sanitize_change_items, sanitize_notes_url};
use crate::update_history::ports::{
    BrewVersionDiffPort, ChangeExtractPort, ExtractRequest, HistoryStorePort, NixVersionPort,
    NotesPort,
};

/// nix/brew の version 差分を統合し、各アプリの生ノートを LLM 抽出して 1 エントリを履歴へ追記する。
///
/// 順序制御の理由: 差分（nix→brew）を先に確定してから各アプリのノート取得・LLM 抽出を行うのは、
/// 差分に現れたアプリだけをノート取得・抽出対象にし、無関係なノート取得を避けるためである。nix 差分は
/// eval ベース化により `nix store diff-closures`（フル closure を 2 回ビルド）ではなく、ci-ref の old/new
/// lock で eval した宣言パッケージの name→version マップ 2 つを取得し、domain の純粋比較
/// （[`diff_versions`]）に通して求める（ビルド/フェッチ不要・数秒）。停止条件は各 port の `Err` 伝播であり、
/// ノート取得不能（`None`）や LLM 未使用（空配列）はフォールバックとして version + URL のみへ縮退させ、
/// record 全体は失敗させない。
///
/// 抽出フェーズ全体の wall-clock 予算（停止条件）: 全件持続 429 のような最悪ケースでは LLM 抽出の 429 リトライ
/// 待機が積み上がり、抽出ループ全体が record job timeout（60分）へ接近・超過しうる（超過すると後続 job（PR 起票）が
/// 止まり無人 nightly が停止する）。これを構造的に防ぐため、各パッケージの抽出前に
/// [`ChangeExtractPort::extract_budget_exhausted`] を問い合わせ、予算超過後は LLM 抽出を呼ばず version-only（変更
/// リスト空・notes_url は保持）へ縮退させる（record は success 継続）。予算超過で skip した件数は最後に 1 行で
/// stderr へ明示する（サイレント切り捨て防止）。予算の計測（時刻・経過）は port 実装（adapter）が担い、application は
/// 「残りを skip して version-only に倒す」停止条件の判断だけを持つ。LLM 出力と生ノート URL は信頼境界外のため、記録前に必ず domain の機械
/// バリデート（host allowlist / 長さ / 件数）を通す。severity / overall の算出は domain（[`build_entry`]）に
/// 委ね、application は素材収集の順序だけを保持する。
pub(crate) fn run_record<V, B, N, X, S>(
    command: RecordCommand,
    nix_versions: &V,
    brew_diff: &B,
    notes: &N,
    extract: &X,
    store: &S,
) -> Result<()>
where
    V: NixVersionPort,
    B: BrewVersionDiffPort,
    N: NotesPort,
    X: ChangeExtractPort,
    S: HistoryStorePort,
{
    // ci-ref の old/new lock で eval した宣言パッケージ name→version マップを取得し、domain の純粋比較で
    // 差分を求める。closure を実体化せず評価時属性だけを比べるため、ビルドは一切走らない。
    let old_versions = nix_versions.old_versions()?;
    let new_versions = nix_versions.new_versions()?;
    let nix_deltas = diff_versions(&old_versions, &new_versions);
    let brew_deltas = brew_diff.diff_brew_versions(&command.old_rev, &command.new_rev)?;
    let deltas = merge_version_deltas(nix_deltas, brew_deltas);

    let mut materials = Vec::with_capacity(deltas.len());
    // 予算超過で LLM 抽出を skip して version-only へ縮退させたパッケージ数。最後に 1 行で件数を明示する
    // （サイレント切り捨て防止）。
    let mut budget_skipped = 0usize;
    // ノート取得・抽出の縮退を可観測にするためのサマリ件数（無人パイプラインがサイレント全滅に気づけるよう、
    // 概要付き＝抽出結果が 1 件以上ついた件数と、version-only＝変更リスト空へ縮退した件数を最後に 1 行で出す）。
    let mut summarized = 0usize;
    let mut version_only = 0usize;
    for delta in deltas {
        // 機械解決でノート取得を先に試みる（AI エージェントへ与える初期材料 + 記録用 notes_url の供給）。
        // 出所（nix/brew）でノート取得先 base / 解決規則が異なるため source を渡す（同一規則で引くと誤った URL
        // になるのを防ぐ）。これは AI 主導の前段ヒントであり、取得不能でも AI はヒント URL から自分で fetch を
        // 試みられる（機械解決は fallback/ヒントとして残す）。
        let seed = notes.fetch_release_notes(
            &delta.name,
            delta.source,
            delta.repo.clone(),
            delta.notes_source.clone(),
            delta.old.clone(),
            delta.new.clone(),
        )?;
        // 記録用 notes_url は機械解決が返した URL を一次に、無ければ changelog/homepage ヒントへ倒す
        // （いずれも記録前に host allowlist で機械バリデートする）。
        let notes_url = sanitize_notes_url(
            seed.as_ref()
                .map(|seed| seed.notes_url.clone())
                .or_else(|| delta.notes_source.clone())
                .or_else(|| delta.homepage.clone()),
        );
        let change_items = if extract.extract_budget_exhausted() {
            // 抽出フェーズ全体の wall-clock 予算を使い切っていたら、残りパッケージは AI 抽出を呼ばず
            // version-only（変更リスト空・URL は保持）へ縮退する。全件持続 429 の最悪ケースで抽出が record job
            // timeout（60分）へ接近・超過し、後続 job（PR 起票）が止まって無人 nightly が停止するのを構造的に防ぐ。
            // 停止条件（残りを skip して version-only に倒す）の判断は application が持ち、予算超過の計測は port が担う。
            budget_skipped += 1;
            Vec::new()
        } else {
            // AI エージェントにヒント（パッケージ名・old→new・homepage/repo/changelog）と seed ノートを渡し、
            // AI 自身に適切なノートを fetch・読解させて構造化変更を抽出させる。SSRF 許可ホスト集合の組み立てと
            // fetch は adapter（port 裏）の責務。LLM 出力は信頼境界外のため、記録前に host/長さ/件数を機械
            // バリデートする。
            let request = ExtractRequest {
                name: delta.name.clone(),
                old: delta.old.clone(),
                new: delta.new.clone(),
                repo: delta.repo.clone(),
                homepage: delta.homepage.clone(),
                changelog: delta.notes_source.clone(),
                seed_notes: seed,
            };
            sanitize_change_items(extract.extract_change_items(&request)?)
        };
        // 可観測サマリの集計: 抽出結果が 1 件以上ついたものを「概要付き」、変更リスト空（ノート取得不能・
        // 抽出 0 件・予算超過縮退）を「version-only」として数える。失敗理由の内訳（auth/rate・budget）は
        // 各 adapter / 予算ログ側で診断済みで、ここでは全体の縮退比率を 1 行で可視化する。
        if change_items.is_empty() {
            version_only += 1;
        } else {
            summarized += 1;
        }
        materials.push(PackageMaterial {
            delta,
            change_items,
            notes_url,
        });
    }
    // 予算超過で抽出を skip したパッケージがあれば件数を 1 行で明示する（サイレント切り捨てなし）。
    if budget_skipped > 0 {
        eprintln!(
            "GitHub Models extract: budget exhausted, {budget_skipped} packages recorded version-only"
        );
    }
    // ノート取得・抽出フェーズの縮退サマリを 1 行で出す（概要付き/version-only の件数）。token 失効・レート
    // 枯渇で全件 version-only に静かに全滅しても、無人パイプラインが CI ログで気づけるようにする
    // （budget-exhausted ログと対称。サイレント全滅防止）。対象 delta が 1 件もない夜は出さない。
    if summarized + version_only > 0 {
        eprintln!("notes: {summarized} packages summarized, {version_only} version-only");
    }

    // append 要否の判定: **rev が前進した（`nixpkgs_old != nixpkgs_new`）夜は materials が空でも append する**。
    // nixpkgs rev の chain link を欠落させると、`r0` に pin されたマシンの catch-up（`select_entries` が
    // `nixpkgs_old == rev` の完全一致で起点を解決する）で起点が見つからず、後続の夜に実際に適用・記録された更新
    // まで含む要約が一切表示されなくなる退行が起きる（catch-up 連続性）。一方で **rev が前進していない
    // （`nixpkgs_old == nixpkgs_new`）かつ materials も空**の夜は、chain link としての意味も差分素材も無い無意味な
    // `packages=[]` エントリを生むだけなので append を skip する。条件をまとめると「append する ⇔ rev 前進あり
    // または materials 非空」。空エントリの利用者表示ノイズ除去は表示側（catch-up 集約が package=0 を畳む）で行う。
    let rev_advanced = command.nixpkgs_old != command.nixpkgs_new;
    if !rev_advanced && materials.is_empty() {
        // rev 前進なし・差分素材なし。chain link にも要約にもならない空エントリは履歴へ残さない。
        return Ok(());
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
    //! record の順序（nix eval diff / brew diff → ノート取得 → LLM 抽出 → サニタイズ → 追記）と
    //! フォールバック（ノート不在で version のみ）・バリデート（不正 URL 破棄）を mockall mock で固定する。

    use std::collections::BTreeMap;

    use super::run_record;
    use crate::update_history::domain::commands::RecordCommand;
    use crate::update_history::domain::diff::{DeltaSource, NixPackage, VersionDelta};
    use crate::update_history::domain::wire::{ChangeCategory, ChangeItem, ChangeKind, Severity};
    use crate::update_history::ports::{
        MockBrewVersionDiffPort, MockChangeExtractPort, MockHistoryStorePort, MockNixVersionPort,
        MockNotesPort, RawReleaseNotes,
    };

    fn command() -> RecordCommand {
        RecordCommand {
            old_rev: "oldrev".to_string(),
            new_rev: "newrev".to_string(),
            nixpkgs_old: "a1b2c3d".to_string(),
            nixpkgs_new: "e4f5g6h".to_string(),
            reference: "darwinConfigurations.ci".to_string(),
            at: "2026-06-05T18:00:11Z".to_string(),
        }
    }

    /// version と repo・notes_source から `NixPackage` を作る。
    fn pkg(version: &str, repo: &str, notes_source: &str) -> NixPackage {
        NixPackage {
            version: version.to_string(),
            repo: repo.to_string(),
            notes_source: notes_source.to_string(),
            homepage: String::new(),
        }
    }

    /// 単一 nix delta（`name`, 1.0→1.1、repo/notes_source 空）を生む old/new eval マップを返す mock を組む。
    fn nix_versions_for(name: &'static str) -> MockNixVersionPort {
        nix_versions_with_repo(name, "")
    }

    /// 単一 nix delta（`name`, 1.0→1.1）で new 側に repo（owner/repo）を持たせる eval マップ mock。
    fn nix_versions_with_repo(name: &'static str, repo: &'static str) -> MockNixVersionPort {
        let mut nix = MockNixVersionPort::new();
        nix.expect_old_versions()
            .returning(move || Ok(BTreeMap::from([(name.to_string(), pkg("1.0", "", ""))])));
        nix.expect_new_versions()
            .returning(move || Ok(BTreeMap::from([(name.to_string(), pkg("1.1", repo, ""))])));
        nix
    }

    /// nix 差分が空（old==new）になる eval マップ mock。
    fn nix_versions_empty() -> MockNixVersionPort {
        let mut nix = MockNixVersionPort::new();
        nix.expect_old_versions().returning(|| Ok(BTreeMap::new()));
        nix.expect_new_versions().returning(|| Ok(BTreeMap::new()));
        nix
    }

    fn brew_delta(name: &str) -> VersionDelta {
        VersionDelta {
            name: name.to_string(),
            old: Some("120".to_string()),
            new: Some("121".to_string()),
            change: ChangeKind::Upgraded,
            source: DeltaSource::BrewTap,
            repo: None,
            notes_source: None,
            homepage: None,
        }
    }

    #[test]
    fn notes_fetch_receives_per_delta_source() -> crate::Result<()> {
        // N5 退行固定: nix delta には NixEval、brew delta には BrewTap が NotesPort へ渡る。
        // これにより adapter は出所別 base（forge / cask レイアウト）で正しい URL を引ける。
        let nix_versions = nix_versions_for("openssl");
        let mut brew_diff = MockBrewVersionDiffPort::new();
        brew_diff
            .expect_diff_brew_versions()
            .returning(|_, _| Ok(vec![brew_delta("firefox")]));

        let mut notes = MockNotesPort::new();
        // nix package openssl は NixEval 出所で引かれる。
        notes
            .expect_fetch_release_notes()
            .withf(|name, source, _, _, _, _| name == "openssl" && *source == DeltaSource::NixEval)
            .times(1)
            .returning(|_, _, _, _, _, _| Ok(None));
        // brew cask firefox は BrewTap 出所で引かれる。
        notes
            .expect_fetch_release_notes()
            .withf(|name, source, _, _, _, _| name == "firefox" && *source == DeltaSource::BrewTap)
            .times(1)
            .returning(|_, _, _, _, _, _| Ok(None));

        // seed ノートが取れなくても AI エージェントはヒント URL から自分で fetch を試みられるため、予算未超過なら
        // 各 delta で抽出が呼ばれる（ここでは AI もノートを見つけられず空配列を返す = version-only へ縮退）。
        let mut extract = MockChangeExtractPort::new();
        extract
            .expect_extract_budget_exhausted()
            .returning(|| false);
        extract
            .expect_extract_change_items()
            .times(2)
            .returning(|_| Ok(Vec::new()));

        let mut store = MockHistoryStorePort::new();
        store
            .expect_append_entry()
            .times(1)
            .withf(|entry| entry.packages.len() == 2)
            .returning(|_| Ok(()));

        run_record(
            command(),
            &nix_versions,
            &brew_diff,
            &notes,
            &extract,
            &store,
        )
    }

    #[test]
    fn record_extracts_sanitizes_and_appends_one_entry() -> crate::Result<()> {
        // nix delta は new 側 repo（owner/repo）を運び、それが NotesPort へ渡る（Releases API 取得経路）。
        let nix_versions = nix_versions_with_repo("openssl", "openssl/openssl");
        let mut brew_diff = MockBrewVersionDiffPort::new();
        brew_diff
            .expect_diff_brew_versions()
            .times(1)
            .returning(|_, _| Ok(Vec::new()));

        let mut notes = MockNotesPort::new();
        notes
            .expect_fetch_release_notes()
            // nix delta なので source = NixEval と new 側 repo が渡ることを withf で固定する
            // （N5 振り分け + nix リリースノート取得元運搬）。
            .withf(|_, source, repo, _, _, _| {
                *source == DeltaSource::NixEval && repo.as_deref() == Some("openssl/openssl")
            })
            .times(1)
            .returning(|_, _, _, _, _, _| {
                Ok(Some(RawReleaseNotes {
                    text: "CVE fix".to_string(),
                    notes_url: "https://github.com/openssl/openssl/releases/tag/v1.1".to_string(),
                }))
            });

        let mut extract = MockChangeExtractPort::new();
        // 予算未超過なら通常どおり抽出する。
        extract
            .expect_extract_budget_exhausted()
            .returning(|| false);
        extract
            .expect_extract_change_items()
            // ExtractRequest が delta のヒント（name・old→new・repo）と seed ノートを運ぶことを固定する。
            .withf(|request| {
                request.name == "openssl"
                    && request.old.as_deref() == Some("1.0")
                    && request.new.as_deref() == Some("1.1")
                    && request.repo.as_deref() == Some("openssl/openssl")
                    && request.seed_notes.as_ref().map(|notes| notes.text.as_str())
                        == Some("CVE fix")
            })
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
            &nix_versions,
            &brew_diff,
            &notes,
            &extract,
            &store,
        )
    }

    #[test]
    fn record_skips_extraction_to_version_only_when_extract_budget_exhausted() -> crate::Result<()>
    {
        // 退行固定（抽出フェーズ wall-clock 予算）: ノートは取得できても抽出フェーズの予算を使い切っていれば、
        // LLM 抽出を呼ばず version-only（変更リスト空・notes_url は保持）へ縮退する。これにより全件持続 429 の
        // 最悪ケースでも record 総時間が record job timeout（60分）内へ構造的に収まる。stop 判断は application、
        // 予算計測は port（mock では予算超過 = true を返す）。
        let nix_versions = nix_versions_with_repo("openssl", "openssl/openssl");
        let mut brew_diff = MockBrewVersionDiffPort::new();
        brew_diff
            .expect_diff_brew_versions()
            .times(1)
            .returning(|_, _| Ok(Vec::new()));

        let mut notes = MockNotesPort::new();
        notes
            .expect_fetch_release_notes()
            .times(1)
            .returning(|_, _, _, _, _, _| {
                Ok(Some(RawReleaseNotes {
                    text: "CVE fix".to_string(),
                    notes_url: "https://github.com/openssl/openssl/releases/tag/v1.1".to_string(),
                }))
            });

        let mut extract = MockChangeExtractPort::new();
        // 予算超過なので各パッケージの抽出前に true を返し、抽出は一度も呼ばれない。
        extract
            .expect_extract_budget_exhausted()
            .times(1)
            .returning(|| true);
        extract.expect_extract_change_items().never();

        let mut store = MockHistoryStorePort::new();
        store
            .expect_append_entry()
            .times(1)
            .withf(|entry| {
                // version-only 縮退: 変更リストは空だが notes_url は保持される。
                entry.packages.len() == 1
                    && entry.packages[0].name == "openssl"
                    && entry.packages[0].change_items.is_empty()
                    && entry.packages[0].notes_url.as_deref()
                        == Some("https://github.com/openssl/openssl/releases/tag/v1.1")
            })
            .returning(|_| Ok(()));

        run_record(
            command(),
            &nix_versions,
            &brew_diff,
            &notes,
            &extract,
            &store,
        )
    }

    #[test]
    fn record_falls_back_to_version_only_when_notes_absent() -> crate::Result<()> {
        // seed ノート取得不能・ヒント URL も無い（repo/homepage/changelog すべて空）なら、AI エージェントは
        // 取得元が無いため空配列を返し、version-only（変更リスト空・notes_url なし）へ縮退する。
        let nix_versions = nix_versions_for("neovim");
        let mut brew_diff = MockBrewVersionDiffPort::new();
        brew_diff
            .expect_diff_brew_versions()
            .returning(|_, _| Ok(Vec::new()));

        let mut notes = MockNotesPort::new();
        // seed ノート取得は試みるが取得不能（None）。
        notes
            .expect_fetch_release_notes()
            .times(1)
            .returning(|_, _, _, _, _, _| Ok(None));
        let mut extract = MockChangeExtractPort::new();
        // 予算未超過なので AI 抽出は呼ばれるが、ヒント URL が無いため AI も空配列を返す。
        extract
            .expect_extract_budget_exhausted()
            .returning(|| false);
        extract
            .expect_extract_change_items()
            .times(1)
            .returning(|_| Ok(Vec::new()));

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
            &nix_versions,
            &brew_diff,
            &notes,
            &extract,
            &store,
        )
    }

    #[test]
    fn record_skips_append_when_rev_unchanged_and_empty() -> crate::Result<()> {
        // N9 退行固定: rev 前進なし（`nixpkgs_old == nixpkgs_new`）かつ差分素材も空の夜は、chain link にも
        // 要約にもならない無意味な `packages=[]` エントリを生むため append を skip する。
        let mut command = command();
        command.nixpkgs_new = command.nixpkgs_old.clone();

        let nix_versions = nix_versions_empty();
        let mut brew_diff = MockBrewVersionDiffPort::new();
        brew_diff
            .expect_diff_brew_versions()
            .times(1)
            .returning(|_, _| Ok(Vec::new()));

        let mut notes = MockNotesPort::new();
        notes.expect_fetch_release_notes().never();
        let mut extract = MockChangeExtractPort::new();
        extract.expect_extract_change_items().never();

        let mut store = MockHistoryStorePort::new();
        // rev 不変・空素材なので append は一切行わない。
        store.expect_append_entry().never();

        run_record(command, &nix_versions, &brew_diff, &notes, &extract, &store)
    }

    #[test]
    fn record_appends_empty_chain_link_when_no_deltas() -> crate::Result<()> {
        // 退行固定（chain 連続性 / N9 の「rev 前進あり空 packages→append」側）: nix eval 差分も brew 差分も空
        // （更新無し）でも、`nixpkgs_old != nixpkgs_new`（rev 前進あり）なら `packages=[]` のエントリを必ず
        // 追記する。これを欠くと r0 に pin されたマシンの catch-up で `select_entries` が起点 rev を解決できず、
        // 後続の実更新まで表示が消える。ノート取得・LLM 抽出は対象 delta が無いため呼ばれない（never）が、
        // append は 1 回行う。
        let nix_versions = nix_versions_empty();
        let mut brew_diff = MockBrewVersionDiffPort::new();
        brew_diff
            .expect_diff_brew_versions()
            .times(1)
            .returning(|_, _| Ok(Vec::new()));

        let mut notes = MockNotesPort::new();
        notes.expect_fetch_release_notes().never();
        let mut extract = MockChangeExtractPort::new();
        extract.expect_extract_change_items().never();

        let mut store = MockHistoryStorePort::new();
        store
            .expect_append_entry()
            .times(1)
            .withf(|entry| {
                // chain link として rev を保持し、packages は空、severity は None。
                entry.nixpkgs_old == "a1b2c3d"
                    && entry.nixpkgs_new == "e4f5g6h"
                    && entry.packages.is_empty()
                    && entry.severity == Severity::None
                    && entry.overall == "0アプリ更新"
            })
            .returning(|_| Ok(()));

        run_record(
            command(),
            &nix_versions,
            &brew_diff,
            &notes,
            &extract,
            &store,
        )
    }
}
