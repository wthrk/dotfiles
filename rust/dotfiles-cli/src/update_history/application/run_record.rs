//! record use case: version 差分を集め、各アプリのリリースノートを LLM で構造化抽出して履歴へ追記する。

use crate::Result;
use crate::update_history::domain::build::{PackageMaterial, build_entry};
use crate::update_history::domain::commands::RecordCommand;
use crate::update_history::domain::diff::{DeltaSource, diff_versions, merge_version_deltas};
use crate::update_history::domain::registry::{NotesOrigin, NotesSourceEntry, NotesSourceRegistry};
use crate::update_history::domain::validate::{
    is_allowed_url, sanitize_change_items, sanitize_notes_url,
};
use crate::update_history::ports::{
    BrewVersionDiffPort, ChangeExtractPort, ExtractOutcome, ExtractRequest, HistoryStorePort,
    NixVersionPort, NotesPort, NotesSourceRegistryPort, RecordDiagnosticsPort,
};

/// `run_record` 実行時の port trait 実装への参照を束ねる runtime/dependency bundle。
///
/// `record` use case は version 差分取得（nix/brew）・ノート取得・LLM 抽出・履歴永続化・provenance レジストリ・
/// 診断出力という cohesive な「履歴記録に必要な外部境界一式」を同時に必要とする。これらを引数で個別に渡すと
/// `run_record` の引数が肥大化するため、`run_*` 関数に閉じた実行時依存束として named fields でまとめる
/// （`Ports` とは命名しない＝port trait 契約そのものと誤認させない。これは実行環境の依存束である）。各 field は
/// 既存 port trait 実装への参照のみを持ち、domain 入出力・summary・policy は持たない（引数数を隠すための無意味な
/// 詰め替えではなく、record が要求する外部境界一式を 1 つの実行環境境界へ寄せたもの）。
pub(crate) struct RecordRuntime<'a, V, B, N, X, S, G, D> {
    /// nix eval 由来の old/new 宣言パッケージ版マップ取得。
    pub(crate) nix_versions: &'a V,
    /// brew tap rev 間の version 差分取得。
    pub(crate) brew_diff: &'a B,
    /// リリースノート取得（出所別解決・再利用 source 直接 fetch）。
    pub(crate) notes: &'a N,
    /// AI エージェントによる構造化変更抽出（予算ゲート付き）。
    pub(crate) extract: &'a X,
    /// 更新履歴 TOML の read/append。
    pub(crate) store: &'a S,
    /// ノート取得元レジストリ（provenance）の read/write。
    pub(crate) registry_store: &'a G,
    /// 縮退・provenance 経路の診断出力。
    pub(crate) diagnostics: &'a D,
}

/// nix/brew の version 差分を統合し、各アプリのノートを **レジストリ参照 → 機械解決 → AI 探索**の順で取得・
/// LLM 抽出して 1 エントリを履歴へ追記し、取得元（provenance）をレジストリへ学習する。
///
/// 順序制御の理由: 差分（nix→brew）を先に確定してから各アプリのノート取得・LLM 抽出を行うのは、
/// 差分に現れたアプリだけをノート取得・抽出対象にし、無関係なノート取得を避けるためである。nix 差分は
/// eval ベース化により `nix store diff-closures`（フル closure を 2 回ビルド）ではなく、ci-ref の old/new
/// lock で eval した宣言パッケージの name→version マップ 2 つを取得し、domain の純粋比較
/// （[`diff_versions`]）に通して求める（ビルド/フェッチ不要・数秒）。停止条件は各 port の `Err` 伝播であり、
/// ノート取得不能（`None`）や LLM 未使用（空配列）はフォールバックとして version + URL のみへ縮退させ、
/// record 全体は失敗させない。
///
/// **ノート取得元レジストリ（学習・再利用。利用者要件 (3)/(4)）**: 各パッケージごとに次の順で notes を得る:
/// 1. **レジストリ参照**: 保存済み `source`（[`NotesOrigin::Mechanical`]/[`NotesOrigin::AiDiscovered`]）が
///    あれば、それを **直接 fetch**（[`NotesPort::fetch_notes_from_source`]・host allowlist 検査つき）し、その
///    seed ノートを抽出 port へ渡す。**機械解決も AI 探索もしない**。fetch 成功で要約できたら provenance は
///    据え置き（origin 維持）。
/// 2. レジストリ未登録／**自己修復**（保存 source の fetch が空/失敗）なら **機械解決**（既存 Releases API
///    range / changelog 解決）。取れたら、その取得元 URL を [`NotesOrigin::Mechanical`] でレジストリへ記録する。
/// 3. 機械解決も不能なら **AI エージェント探索**（既存 agent loop）。AI が fetch して有効ノートを得て採用した
///    取得元 URL を [`NotesOrigin::AiDiscovered`] でレジストリへ記録する（[`ExtractOutcome::source_url`] 経路）。
///
/// **GitHub Models レート消費の逓減（利用者要件 4）**: 抽出 port（adapter）は seed の有無で呼び出し回数を
/// 変える。フロー 1/2 で seed ノートが取れたパッケージは **ツール探索なしの要約のみ 1 回**で済み、フロー 3
/// （seed 無し＝未知ノート）だけが tool-use 探索（最大 MAX_TOOL_ITERATIONS+1 回の model 呼び出し）を行う。
/// よって registry が回を追って埋まる（registry 参照 hit と機械解決 hit が増える）ほど、GitHub Models 呼び出し
/// 回数の総和が実際に逓減する。これが「再利用でレート消費を逓減」の核である。
/// 4. いずれも不能なら version-only。**保存済みの有効 source が無いときだけ** `origin=none` を記録して次回も
///    探索対象に戻す（取得元が後から現れる可能性に追従。設計判断: 空エントリを残すより「探索済みだが未発見」を
///    明示する方が再探索の根拠が残る）。**保存済みの有効 source が在る**のに全経路が空を返した場合（保存 source の
///    fetch が一時的に失敗し、同 run の機械解決・AI 探索も空だった）は、その既存エントリを **`origin=none` で
///    上書きせず保持**する（finding 3374863459。1 回の一時失敗で有効な取得元を失い次回も探索/LLM 消費へ戻る退行を
///    防ぐ。新しい有効 source を得た時だけフロー 2/3 が上書きする）。
///
/// **自己修復**: レジストリの保存 source を fetch して空/失敗なら、機械解決 → AI 探索へフォールバックし、成功
/// した新ソースでレジストリを更新する（プロジェクトが changelog を移動した等に追従する）。全経路が空を返した
/// 一時失敗時は既存 source を保持し（上記フロー 4）、有効な取得元を 1 回の不調で失わない。
///
/// **セキュリティ（SSRF/学習境界）**: レジストリへ書く URL は **記録前に必ず host allowlist（[`is_allowed_url`]）
/// で機械検証**し、許可外 host の source は学習しない（`origin=none` へ倒す）。これにより次回フロー 1 の再利用でも
/// 許可外 URL を fetch しない。AI 採用 URL は SSRF 検査を通った fetch のものだが、レジストリは repo 管理で人手改変も
/// ありうるため、再利用 fetch（adapter）でも host allowlist を再適用する（二重防御）。
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
///
/// **provenance 可観測性**: ノート取得・抽出フェーズの縮退サマリに加え、レジストリ hit 件数・機械解決件数・
/// AI 探索件数を最後に 1 行で stderr へ出す（どの経路でノートを得たかを CI ログで可視化。サイレント化しない）。
pub(crate) fn run_record<V, B, N, X, S, G, D>(
    command: RecordCommand,
    runtime: &RecordRuntime<'_, V, B, N, X, S, G, D>,
) -> Result<()>
where
    V: NixVersionPort,
    B: BrewVersionDiffPort,
    N: NotesPort,
    X: ChangeExtractPort,
    S: HistoryStorePort,
    G: NotesSourceRegistryPort,
    D: RecordDiagnosticsPort,
{
    // 実行時依存は runtime bundle から取り出す（引数肥大化を避けつつ、cohesive な外部境界一式へ寄せる）。
    let RecordRuntime {
        nix_versions,
        brew_diff,
        notes,
        extract,
        store,
        registry_store,
        diagnostics,
    } = runtime;

    // ci-ref の old/new lock で eval した宣言パッケージ name→version マップを取得し、domain の純粋比較で
    // 差分を求める。closure を実体化せず評価時属性だけを比べるため、ビルドは一切走らない。
    let old_versions = nix_versions.old_versions()?;
    let new_versions = nix_versions.new_versions()?;
    let nix_deltas = diff_versions(&old_versions, &new_versions);
    let brew_deltas = brew_diff.diff_brew_versions(&command.old_rev, &command.new_rev)?;
    let deltas = merge_version_deltas(nix_deltas, brew_deltas);

    // レジストリ（provenance の学習・再利用）を 1 回読み出す。フロー 1 の最優先参照に使い、更新があれば
    // 最後に 1 回書き戻す（決定論・名前昇順は domain/adapter が保証）。読み出し失敗は record 全体の Err として
    // 伝播させる（レジストリ破損を黙殺しない）。
    let mut registry = registry_store.read_registry()?;
    let mut registry_dirty = false;

    let mut materials = Vec::with_capacity(deltas.len());
    // 予算超過で LLM 抽出を skip して version-only へ縮退させたパッケージ数。最後に 1 行で件数を明示する
    // （サイレント切り捨て防止）。
    let mut budget_skipped = 0usize;
    // ノート取得・抽出の縮退を可観測にするためのサマリ件数（無人パイプラインがサイレント全滅に気づけるよう、
    // 概要付き＝抽出結果が 1 件以上ついた件数と、version-only＝変更リスト空へ縮退した件数を最後に 1 行で出す）。
    let mut summarized = 0usize;
    let mut version_only = 0usize;
    // provenance 経路の内訳（どこからノートを得たか）を CI ログで可視化するための件数。
    let mut registry_hits = 0usize;
    let mut mechanical_found = 0usize;
    let mut ai_discovered = 0usize;
    for delta in deltas {
        // フロー 1（レジストリ参照）を最優先する。保存済み有効 source があれば直接 fetch して再利用し、機械
        // 解決・AI 探索を skip する（再探索しない＝レート逓減）。fetch が空/失敗なら自己修復として機械→AI へ倒す。
        // `reused` が `Some` のとき：seed ノート + 記録 URL を持ち、AI には探索させず seed の要約だけさせる。
        let saved_source = registry
            .lookup(&delta.name, delta.source)
            .and_then(|entry| entry.reusable_source())
            // レジストリは repo 管理で人手改変もありうるため、再利用前にも host allowlist を再適用する（二重防御）。
            .filter(|url| is_allowed_url(url))
            .map(str::to_string);
        let reused = match saved_source.as_deref() {
            Some(url) => notes
                .fetch_notes_from_source(url)?
                .map(|notes| (url.to_string(), notes)),
            None => None,
        };

        // フロー 2（機械解決）: レジストリ未登録 or 自己修復（再利用 fetch 失敗）なら機械解決を試みる。出所
        // （nix/brew）で取得規則が異なるため source を渡す。取れたら origin=mechanical 学習候補にする。
        let mechanical = match &reused {
            Some(_) => None,
            None => notes.fetch_release_notes(
                &delta.name,
                delta.source,
                delta.repo.clone(),
                delta.notes_source.clone(),
                delta.old.clone(),
                delta.new.clone(),
            )?,
        };

        // 抽出へ渡す seed ノートと記録 URL を確定する。reused（フロー 1）→ 再利用ノート、mechanical（フロー 2）
        // → 機械解決ノート、いずれも無ければ None（AI がヒント URL から自分で fetch する＝フロー 3）。
        let (seed, resolved_notes_url) = match (&reused, &mechanical) {
            (Some((url, notes_text)), _) => (Some(notes_text.clone()), Some(url.clone())),
            (None, Some(notes_text)) => {
                (Some(notes_text.clone()), Some(notes_text.notes_url.clone()))
            }
            (None, None) => (None, None),
        };

        // brew cask の探索ヒント（finding 3374863454）: brew delta は repo/homepage/changelog ヒントを運ばず、
        // cask `.rb` 定義そのものは seed にしない（定義ファイルを summarize_only で要約しない）。seed が無い
        // brew delta では cask `.rb` から homepage（無ければ url）を **探索ヒント** として取り出し、AI tool-use
        // 探索（agent_loop）の fetch 許可ホスト基底にする。nix delta や seed が取れた brew delta では呼ばない。
        let brew_homepage_hint = if delta.source == DeltaSource::BrewTap && seed.is_none() {
            notes.resolve_brew_notes_hint(&delta.name)?
        } else {
            None
        };

        // 単一の AI 抽出（予算ゲートつき）: 解決した seed があれば AI はそれを **ツールを与えず 1 回だけ要約**し
        // （探索しない＝GitHub Models 呼び出しは 1 回）、無ければヒント URL から自分で fetch して探索する
        // （フロー 3＝tool-use ループで最大 MAX_TOOL_ITERATIONS+1 回の model 呼び出し）。経路分岐は port 実装
        // （adapter）が seed の有無で行う。outcome は構造化変更 + AI が採用した取得元 URL を運ぶ。registry/機械解決で
        // seed が取れるパッケージは 1 回化されるため、registry が埋まるほど GitHub Models のレート消費が回を追って
        // 逓減する（未知ノートだけが探索＝最大 MAX_TOOL_ITERATIONS+1 回。利用者要件 4）。
        let outcome = if extract.extract_budget_exhausted() {
            // 抽出フェーズの wall-clock 予算超過: AI を呼ばず version-only（変更リスト空・URL は保持）へ縮退する。
            // 全件持続 429 の最悪ケースで抽出が record job timeout（60分）へ接近・超過し、後続 job（PR 起票）が
            // 止まって無人 nightly が停止するのを構造的に防ぐ。停止判断は application、予算計測は port。
            budget_skipped += 1;
            ExtractOutcome::default()
        } else {
            let request = ExtractRequest {
                name: delta.name.clone(),
                old: delta.old.clone(),
                new: delta.new.clone(),
                repo: delta.repo.clone(),
                // brew cask は repo/homepage を運ばないため、cask `.rb` から取り出した探索ヒント（homepage/url）を
                // homepage として与え、agent_loop が実ノートを探せるようにする（無ければ delta の homepage）。
                homepage: brew_homepage_hint
                    .clone()
                    .or_else(|| delta.homepage.clone()),
                changelog: delta.notes_source.clone(),
                seed_notes: seed,
            };
            extract.extract_change_items(&request)?
        };
        // LLM 出力は信頼境界外のため、記録前に host/長さ/件数を機械バリデートする。
        let change_items = sanitize_change_items(outcome.items);

        // provenance を確定して学習する（フロー別。利用者要件 (3)/(4)）:
        // - フロー 1（reused）: origin 据え置き。レジストリを書き換えない（再探索しない・据え置きを保つ）。
        // - フロー 2（mechanical）: 機械解決の取得元 URL を origin=mechanical で学習。
        // - フロー 3（AI 採用 URL あり）: AI が採用した取得元 URL を origin=ai-discovered で学習。
        // - いずれも取得元未確定: origin=none を学習し、次回も探索対象に戻す（取得元が後から現れる可能性に追従）。
        if reused.is_some() {
            registry_hits += 1;
        } else if let Some(mech) = &mechanical {
            mechanical_found += 1;
            // 再利用 source に学習してよいのは **raw 再取得できる URL（`refetch_url`）だけ**である（finding
            // 3369076722）。Releases API range/tag のように `notes_url` が表示用 HTML ページで raw 再取得しても
            // 同じ本文が返らない取得は `refetch_url` が `None` であり、その場合は再利用 source を学習せず
            // origin=none を記録して次回も機械解決し直す（HTML ページを seed に再利用して要約が空/不正確に
            // なるのを防ぐ）。`refetch_url` がある（raw changelog / cask `.rb`）ときだけ origin=mechanical で学習する。
            let provenance = match &mech.refetch_url {
                Some(refetch_url) => NotesSourceEntry {
                    source: Some(refetch_url.clone()),
                    origin: NotesOrigin::Mechanical,
                    discovered_at: Some(command.at.clone()),
                    note: None,
                },
                None => NotesSourceEntry {
                    source: None,
                    origin: NotesOrigin::None,
                    discovered_at: Some(command.at.clone()),
                    note: None,
                },
            };
            learn_provenance(
                &mut registry,
                &mut registry_dirty,
                &delta.name,
                delta.source,
                provenance,
            );
        } else if let Some(source_url) = &outcome.source_url {
            ai_discovered += 1;
            let provenance = NotesSourceEntry {
                source: Some(source_url.clone()),
                origin: NotesOrigin::AiDiscovered,
                discovered_at: Some(command.at.clone()),
                note: None,
            };
            learn_provenance(
                &mut registry,
                &mut registry_dirty,
                &delta.name,
                delta.source,
                provenance,
            );
        } else if registry
            .lookup(&delta.name, delta.source)
            .and_then(NotesSourceEntry::reusable_source)
            .is_some()
        {
            // **全経路（保存 source の再取得・機械解決・AI 探索）が source を返せなかったが、レジストリには
            // 有効な保存 source が既に在る**（finding 3374863459）。これは保存 source の fetch がネットワーク不調や
            // レート制限で **一時的に** 失敗し、同じ run の機械解決・AI 探索も取得元を返せなかったケースである。
            // ここで `record()`（既存エントリを置換）で origin=none を上書きすると、有効だった取得元を 1 回の
            // 一時失敗で失い、次回以降も registry hit せず探索/LLM 消費へ戻ってしまう。よって **既存の有効
            // エントリは上書きせず保持**し、registry を dirty にしない（次回フロー 1 の再利用機会を温存する）。
            // 新たな有効 source を得た時だけ上書きするのは上の mechanical/ai-discovered 分岐が担う。
        } else {
            // 機械解決も AI 採用取得元も無く、保存済みの有効 source も無い。origin=none を学習して次回も探索
            // 対象に戻す（取得元が後から現れる可能性に追従。空エントリを残すより「探索済みだが未発見」を明示する）。
            let provenance = NotesSourceEntry {
                source: None,
                origin: NotesOrigin::None,
                discovered_at: Some(command.at.clone()),
                note: None,
            };
            learn_provenance(
                &mut registry,
                &mut registry_dirty,
                &delta.name,
                delta.source,
                provenance,
            );
        }

        // 記録用 notes_url は解決経路が返した URL を一次に（AI 採用 URL も含む）、無ければ changelog/homepage
        // ヒントへ倒す（いずれも記録前に host allowlist で機械バリデートする）。
        let notes_url = sanitize_notes_url(
            resolved_notes_url
                .or_else(|| outcome.source_url.clone())
                .or_else(|| delta.notes_source.clone())
                .or_else(|| delta.homepage.clone()),
        );

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
    // 予算超過で抽出を skip したパッケージがあれば件数を診断する（サイレント切り捨てなし）。具体的な出力先・
    // 整形は diagnostics port（adapter = stderr）に閉じ、application は「何を観測させるか」だけを決める。
    if budget_skipped > 0 {
        diagnostics.report_budget_skipped(budget_skipped);
    }
    // ノート取得・抽出フェーズの縮退サマリ（概要付き/version-only の件数）と provenance 経路内訳を診断する。
    // token 失効・レート枯渇で全件 version-only に静かに全滅しても、また registry-reused/mechanical が回を追って
    // 増え ai-discovered が新規/未知/自己修復へ収束する（レート逓減）ことも、無人パイプラインが CI ログで観測
    // できるようにする。対象 delta が 1 件もない夜は出さない（出力先・整形は adapter の責務）。
    if summarized + version_only > 0 {
        diagnostics.report_notes_summary(
            summarized,
            version_only,
            registry_hits,
            mechanical_found,
            ai_discovered,
        );
    }

    // レジストリに更新があれば 1 回だけ書き戻す（決定論・名前昇順は domain/adapter が保証）。書き戻し先は
    // nightly が commit する `docs/update-history/**` 内なので、レジストリも同経路で repo に入り次回 record が
    // 参照できる（再利用でレート逓減）。
    if registry_dirty {
        registry_store.write_registry(&registry)?;
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

/// 確定した provenance をサニタイズしてレジストリへ学習し、更新フラグを立てる。
///
/// `record`（記録直前）が許可外 source を学習しないよう [`sanitize_provenance`] を通してからレジストリへ
/// upsert する。同一パッケージの既存エントリは上書きする（自己修復で取得元が移動したプロジェクトに追従）。
/// `dirty` を `true` にして、ループ後にレジストリを 1 回だけ書き戻すべきことを示す。これは「どの provenance を
/// いつ学習するか」という use case orchestration の一部であり、origin マッピングは domain 値の構築に限る。
fn learn_provenance(
    registry: &mut NotesSourceRegistry,
    dirty: &mut bool,
    name: &str,
    source: DeltaSource,
    provenance: NotesSourceEntry,
) {
    registry.record(name, source, sanitize_provenance(provenance));
    *dirty = true;
}

/// 学習する provenance を記録前に host allowlist で機械サニタイズする（許可外 source を学習しない）。
///
/// `source` が許可ホスト https でなければ source を捨てて [`NotesOrigin::None`] へ倒す（許可外 URL を
/// レジストリへ学習せず、次回フロー 1 で fetch しないため）。`origin=none`（source なし）はそのまま通す。
/// これは「許可外 source を学習しない」という業務規則の適用であり、record が記録直前に適用する。
fn sanitize_provenance(entry: NotesSourceEntry) -> NotesSourceEntry {
    match entry.source {
        // 許可ホスト https の source はそのまま学習する。
        Some(ref url) if is_allowed_url(url) => entry,
        // 許可外 host の source は学習しない（origin=none・source なしへ倒す）。
        Some(_) => NotesSourceEntry {
            source: None,
            origin: NotesOrigin::None,
            discovered_at: entry.discovered_at,
            note: entry.note,
        },
        // source 無し（origin=none 等）はそのまま。
        None => entry,
    }
}

#[cfg(test)]
mod tests {
    //! record の順序（レジストリ参照 → 機械解決 → AI 探索 → サニタイズ → 追記）と provenance 学習
    //! （mechanical/ai-discovered/none・許可外 source 不学習）・自己修復・再利用（hit→再探索しない）を
    //! mockall mock で hermetic に固定する。

    use std::collections::BTreeMap;

    use super::run_record;
    use crate::update_history::domain::commands::RecordCommand;
    use crate::update_history::domain::diff::{DeltaSource, NixPackage, VersionDelta};
    use crate::update_history::domain::registry::{
        NotesOrigin, NotesSourceEntry, NotesSourceRegistry,
    };
    use crate::update_history::domain::wire::{ChangeCategory, ChangeItem, ChangeKind, Severity};
    use crate::update_history::ports::{
        ExtractOutcome, MockBrewVersionDiffPort, MockChangeExtractPort, MockHistoryStorePort,
        MockNixVersionPort, MockNotesPort, MockNotesSourceRegistryPort, MockRecordDiagnosticsPort,
        RawReleaseNotes,
    };

    /// 診断 port の mock を作る（観測専用のため任意回受理する。件数の正確値は本体ロジックの責務外）。
    fn diagnostics() -> MockRecordDiagnosticsPort {
        let mut diagnostics = MockRecordDiagnosticsPort::new();
        diagnostics.expect_report_budget_skipped().returning(|_| ());
        diagnostics
            .expect_report_notes_summary()
            .returning(|_, _, _, _, _| ());
        diagnostics
    }

    /// 各 mock port を runtime bundle へ束ねて `run_record` を駆動するテストヘルパ。
    ///
    /// 診断 port は観測専用のため、ここで生成した受理 mock を使う（各テストは記録ロジックの port 駆動だけを
    /// 検証し、診断出力件数は検証対象外）。bundle 構築の定型を 1 か所へ閉じる。
    fn run_record_with(
        command: RecordCommand,
        nix_versions: &MockNixVersionPort,
        brew_diff: &MockBrewVersionDiffPort,
        notes: &MockNotesPort,
        extract: &MockChangeExtractPort,
        store: &MockHistoryStorePort,
        registry_store: &MockNotesSourceRegistryPort,
    ) -> crate::Result<()> {
        let diagnostics = diagnostics();
        let runtime = super::RecordRuntime {
            nix_versions,
            brew_diff,
            notes,
            extract,
            store,
            registry_store,
            diagnostics: &diagnostics,
        };
        run_record(command, &runtime)
    }

    /// 空レジストリを返し、`write_registry` を任意回（学習が起きれば 1 回）受ける registry mock。
    ///
    /// 既存テスト（レジストリ未登録経路）はこの空レジストリを使う。provenance 学習で書き戻しが走るため
    /// `write_registry` は受理する（回数は問わない＝学習有無に依存しないテストで使える）。
    fn empty_registry_store() -> MockNotesSourceRegistryPort {
        let mut registry = MockNotesSourceRegistryPort::new();
        registry
            .expect_read_registry()
            .returning(|| Ok(NotesSourceRegistry::default()));
        registry.expect_write_registry().returning(|_| Ok(()));
        registry
    }

    /// `extract_change_items` の戻り値 [`ExtractOutcome`] を変更リストのみ（採用取得元なし）で作る helper。
    fn outcome(items: Vec<ChangeItem>) -> ExtractOutcome {
        ExtractOutcome {
            items,
            source_url: None,
        }
    }

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
        // brew は cask `.rb` を seed にせず、seed 無し brew delta では探索ヒント（homepage/url）を解決する
        // （finding 3374863454）。ここではヒントも無し（None）として agent_loop へ渡す。
        notes
            .expect_resolve_brew_notes_hint()
            .withf(|name| name == "firefox")
            .times(1)
            .returning(|_| Ok(None));

        // seed ノートが取れなくても AI エージェントはヒント URL から自分で fetch を試みられるため、予算未超過なら
        // 各 delta で抽出が呼ばれる（ここでは AI もノートを見つけられず空配列を返す = version-only へ縮退）。
        let mut extract = MockChangeExtractPort::new();
        extract
            .expect_extract_budget_exhausted()
            .returning(|| false);
        extract
            .expect_extract_change_items()
            .times(2)
            .returning(|_| Ok(outcome(Vec::new())));

        let mut store = MockHistoryStorePort::new();
        store
            .expect_append_entry()
            .times(1)
            .withf(|entry| entry.packages.len() == 2)
            .returning(|_| Ok(()));

        run_record_with(
            command(),
            &nix_versions,
            &brew_diff,
            &notes,
            &extract,
            &store,
            &empty_registry_store(),
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
                    // Releases API 由来は raw 再取得できないため refetch_url なし（記録 notes_url は表示用）。
                    refetch_url: None,
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
                Ok(outcome(vec![
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
                ]))
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

        run_record_with(
            command(),
            &nix_versions,
            &brew_diff,
            &notes,
            &extract,
            &store,
            &empty_registry_store(),
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
                    // Releases API 由来は raw 再取得できないため refetch_url なし（記録 notes_url は表示用）。
                    refetch_url: None,
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

        run_record_with(
            command(),
            &nix_versions,
            &brew_diff,
            &notes,
            &extract,
            &store,
            &empty_registry_store(),
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
            .returning(|_| Ok(outcome(Vec::new())));

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

        run_record_with(
            command(),
            &nix_versions,
            &brew_diff,
            &notes,
            &extract,
            &store,
            &empty_registry_store(),
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

        run_record_with(
            command,
            &nix_versions,
            &brew_diff,
            &notes,
            &extract,
            &store,
            &empty_registry_store(),
        )
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

        run_record_with(
            command(),
            &nix_versions,
            &brew_diff,
            &notes,
            &extract,
            &store,
            &empty_registry_store(),
        )
    }

    /// nix delta（`name`, 1.0→1.1、new 側 repo）を生む old/new eval マップ + brew 空 diff の標準セットを返す。
    fn nix_only_diff(
        name: &'static str,
        repo: &'static str,
    ) -> (MockNixVersionPort, MockBrewVersionDiffPort) {
        let nix_versions = nix_versions_with_repo(name, repo);
        let mut brew_diff = MockBrewVersionDiffPort::new();
        brew_diff
            .expect_diff_brew_versions()
            .returning(|_, _| Ok(Vec::new()));
        (nix_versions, brew_diff)
    }

    #[test]
    fn registry_hit_reuses_saved_source_without_re_searching() -> crate::Result<()> {
        // 退行固定（再利用フロー 1）: レジストリに保存済み source があれば、それを直接 fetch
        // （fetch_notes_from_source）して再利用し、機械解決（fetch_release_notes）も AI 探索も行わない
        // （再探索しない＝レート逓減）。provenance は据え置きなので write_registry も呼ばない。
        let (nix_versions, brew_diff) = nix_only_diff("openssl", "openssl/openssl");

        let mut notes = MockNotesPort::new();
        // 保存 source を直接 fetch して再利用する。
        notes
            .expect_fetch_notes_from_source()
            .withf(|url| url == "https://github.com/openssl/openssl/releases")
            .times(1)
            .returning(|url| {
                Ok(Some(RawReleaseNotes {
                    text: "reused notes".to_string(),
                    notes_url: url.to_string(),
                    refetch_url: Some(url.to_string()),
                }))
            });
        // 機械解決は一切呼ばれない（再探索しない）。
        notes.expect_fetch_release_notes().never();

        let mut extract = MockChangeExtractPort::new();
        extract
            .expect_extract_budget_exhausted()
            .returning(|| false);
        // 再利用ノートを seed に受け取り、AI は探索せず要約する（source_url は無し＝provenance 据え置き）。
        extract
            .expect_extract_change_items()
            .withf(|request| {
                request.seed_notes.as_ref().map(|n| n.text.as_str()) == Some("reused notes")
            })
            .times(1)
            .returning(|_| {
                Ok(outcome(vec![ChangeItem {
                    category: ChangeCategory::Fix,
                    text: "修正".to_string(),
                    ref_url: None,
                }]))
            });

        let mut store = MockHistoryStorePort::new();
        store
            .expect_append_entry()
            .times(1)
            .withf(|entry| {
                entry.packages[0].notes_url.as_deref()
                    == Some("https://github.com/openssl/openssl/releases")
            })
            .returning(|_| Ok(()));

        // レジストリ hit: openssl に origin=mechanical の保存 source。再探索しないので write は呼ばれない。
        let mut registry = MockNotesSourceRegistryPort::new();
        registry.expect_read_registry().returning(|| {
            let mut r = NotesSourceRegistry::default();
            r.record(
                "openssl",
                DeltaSource::NixEval,
                NotesSourceEntry {
                    source: Some("https://github.com/openssl/openssl/releases".to_string()),
                    origin: NotesOrigin::Mechanical,
                    discovered_at: None,
                    note: None,
                },
            );
            Ok(r)
        });
        registry.expect_write_registry().never();

        run_record_with(
            command(),
            &nix_versions,
            &brew_diff,
            &notes,
            &extract,
            &store,
            &registry,
        )
    }

    #[test]
    fn mechanical_resolution_records_origin_mechanical() -> crate::Result<()> {
        // 退行固定（フロー 2 学習）: レジストリ未登録で機械解決が raw 再取得可能な URL（refetch_url）で取れたら、
        // その **refetch_url** を origin=mechanical でレジストリへ記録する（finding 3369076722: 表示用 notes_url
        // でなく raw 再取得 URL を学習する）。ここでは raw changelog（refetch 可）を機械解決が返す。
        let (nix_versions, brew_diff) = nix_only_diff("openssl", "openssl/openssl");

        let mut notes = MockNotesPort::new();
        notes
            .expect_fetch_release_notes()
            .times(1)
            .returning(|_, _, _, _, _, _| {
                Ok(Some(RawReleaseNotes {
                    text: "notes".to_string(),
                    // 表示用 URL（人間が辿るページ）。
                    notes_url: "https://github.com/openssl/openssl/releases/tag/v1.1".to_string(),
                    // raw 再取得できる URL（raw changelog）。これが provenance の再利用 source に学習される。
                    refetch_url: Some(
                        "https://raw.githubusercontent.com/openssl/openssl/v1.1/CHANGELOG.md"
                            .to_string(),
                    ),
                }))
            });

        let mut extract = MockChangeExtractPort::new();
        extract
            .expect_extract_budget_exhausted()
            .returning(|| false);
        extract
            .expect_extract_change_items()
            .returning(|_| Ok(outcome(Vec::new())));

        let mut store = MockHistoryStorePort::new();
        store.expect_append_entry().returning(|_| Ok(()));

        let mut registry = MockNotesSourceRegistryPort::new();
        registry
            .expect_read_registry()
            .returning(|| Ok(NotesSourceRegistry::default()));
        registry
            .expect_write_registry()
            .times(1)
            .withf(|r| {
                r.lookup("openssl", DeltaSource::NixEval).is_some_and(|e| {
                    e.origin == NotesOrigin::Mechanical
                        // 表示用 notes_url ではなく refetch_url（raw changelog）を学習する（finding 3369076722）。
                        && e.source.as_deref()
                            == Some("https://raw.githubusercontent.com/openssl/openssl/v1.1/CHANGELOG.md")
                })
            })
            .returning(|_| Ok(()));

        run_record_with(
            command(),
            &nix_versions,
            &brew_diff,
            &notes,
            &extract,
            &store,
            &registry,
        )
    }

    #[test]
    fn mechanical_without_refetch_url_records_origin_none_not_poisoned_page() -> crate::Result<()> {
        // 退行固定（finding 3369076722）: 機械解決がノートを返しても **raw 再取得できる URL（refetch_url）が
        // 無い**（Releases API range/tag のように notes_url が表示用 HTML ページ）場合、その表示用 URL を再利用
        // source に学習せず origin=none を記録する。これにより次回フロー 1 で HTML ページを raw curl して seed に
        // 再利用し、要約が空/不正確になるのを防ぐ（次回も機械解決し直す）。
        let (nix_versions, brew_diff) = nix_only_diff("openssl", "openssl/openssl");

        let mut notes = MockNotesPort::new();
        notes
            .expect_fetch_release_notes()
            .times(1)
            .returning(|_, _, _, _, _, _| {
                Ok(Some(RawReleaseNotes {
                    text: "range notes".to_string(),
                    // 表示用 HTML ページ（Releases API range 由来）。raw 再取得で同じ本文は返らない。
                    notes_url: "https://github.com/openssl/openssl/releases".to_string(),
                    refetch_url: None,
                }))
            });

        let mut extract = MockChangeExtractPort::new();
        extract
            .expect_extract_budget_exhausted()
            .returning(|| false);
        extract
            .expect_extract_change_items()
            .returning(|_| Ok(outcome(Vec::new())));

        let mut store = MockHistoryStorePort::new();
        // 記録される notes_url は表示用 URL のまま（履歴の表示には HTML ページを残してよい）。
        store
            .expect_append_entry()
            .times(1)
            .withf(|entry| {
                entry.packages[0].notes_url.as_deref()
                    == Some("https://github.com/openssl/openssl/releases")
            })
            .returning(|_| Ok(()));

        let mut registry = MockNotesSourceRegistryPort::new();
        registry
            .expect_read_registry()
            .returning(|| Ok(NotesSourceRegistry::default()));
        // refetch_url が無いので再利用 source を学習せず origin=none（表示用 HTML ページを source にしない）。
        registry
            .expect_write_registry()
            .times(1)
            .withf(|r| {
                r.lookup("openssl", DeltaSource::NixEval)
                    .is_some_and(|e| e.origin == NotesOrigin::None && e.source.is_none())
            })
            .returning(|_| Ok(()));

        run_record_with(
            command(),
            &nix_versions,
            &brew_diff,
            &notes,
            &extract,
            &store,
            &registry,
        )
    }

    #[test]
    fn ai_discovered_source_records_origin_ai_discovered() -> crate::Result<()> {
        // 退行固定（フロー 3 学習）: 機械解決が空で AI が取得元 URL を採用したら、その URL を
        // origin=ai-discovered でレジストリへ記録する。
        let (nix_versions, brew_diff) = nix_only_diff("neovim", "neovim/neovim");

        let mut notes = MockNotesPort::new();
        // 機械解決は空（None）→ AI 探索へ倒す。
        notes
            .expect_fetch_release_notes()
            .times(1)
            .returning(|_, _, _, _, _, _| Ok(None));

        let mut extract = MockChangeExtractPort::new();
        extract
            .expect_extract_budget_exhausted()
            .returning(|| false);
        // AI が採用取得元 URL を返す（source_url）。
        extract
            .expect_extract_change_items()
            .times(1)
            .returning(|_| {
                Ok(ExtractOutcome {
                    items: vec![ChangeItem {
                        category: ChangeCategory::Feature,
                        text: "新機能".to_string(),
                        ref_url: None,
                    }],
                    source_url: Some("https://github.com/neovim/neovim/releases".to_string()),
                })
            });

        let mut store = MockHistoryStorePort::new();
        store.expect_append_entry().returning(|_| Ok(()));

        let mut registry = MockNotesSourceRegistryPort::new();
        registry
            .expect_read_registry()
            .returning(|| Ok(NotesSourceRegistry::default()));
        registry
            .expect_write_registry()
            .times(1)
            .withf(|r| {
                r.lookup("neovim", DeltaSource::NixEval).is_some_and(|e| {
                    e.origin == NotesOrigin::AiDiscovered
                        && e.source.as_deref() == Some("https://github.com/neovim/neovim/releases")
                })
            })
            .returning(|_| Ok(()));

        run_record_with(
            command(),
            &nix_versions,
            &brew_diff,
            &notes,
            &extract,
            &store,
            &registry,
        )
    }

    #[test]
    fn unresolved_notes_record_origin_none() -> crate::Result<()> {
        // 退行固定（フロー 4 学習）: 機械解決も AI 採用取得元も無ければ origin=none を記録し、次回も探索対象に戻す。
        let (nix_versions, brew_diff) = nix_only_diff("zlib", "");

        let mut notes = MockNotesPort::new();
        notes
            .expect_fetch_release_notes()
            .times(1)
            .returning(|_, _, _, _, _, _| Ok(None));

        let mut extract = MockChangeExtractPort::new();
        extract
            .expect_extract_budget_exhausted()
            .returning(|| false);
        // AI も取得元を採用できない（source_url なし・空変更）。
        extract
            .expect_extract_change_items()
            .returning(|_| Ok(outcome(Vec::new())));

        let mut store = MockHistoryStorePort::new();
        store.expect_append_entry().returning(|_| Ok(()));

        let mut registry = MockNotesSourceRegistryPort::new();
        registry
            .expect_read_registry()
            .returning(|| Ok(NotesSourceRegistry::default()));
        registry
            .expect_write_registry()
            .times(1)
            .withf(|r| {
                r.lookup("zlib", DeltaSource::NixEval)
                    .is_some_and(|e| e.origin == NotesOrigin::None && e.source.is_none())
            })
            .returning(|_| Ok(()));

        run_record_with(
            command(),
            &nix_versions,
            &brew_diff,
            &notes,
            &extract,
            &store,
            &registry,
        )
    }

    #[test]
    fn self_heals_when_saved_source_fetch_fails() -> crate::Result<()> {
        // 退行固定（自己修復）: レジストリ保存 source の直接 fetch が空/失敗（None）なら、機械解決へフォールバック
        // し、成功した新ソースで origin=mechanical へレジストリを更新する（取得元移動に追従）。
        let (nix_versions, brew_diff) = nix_only_diff("openssl", "openssl/openssl");

        let mut notes = MockNotesPort::new();
        // 保存 source の再利用 fetch が失敗（None）。
        notes
            .expect_fetch_notes_from_source()
            .times(1)
            .returning(|_| Ok(None));
        // 自己修復で機械解決を試みて新ソース（raw 再取得可能な refetch_url）を得る。
        notes
            .expect_fetch_release_notes()
            .times(1)
            .returning(|_, _, _, _, _, _| {
                Ok(Some(RawReleaseNotes {
                    text: "fresh notes".to_string(),
                    notes_url: "https://github.com/openssl/openssl/releases/tag/v1.1".to_string(),
                    refetch_url: Some(
                        "https://raw.githubusercontent.com/openssl/openssl/v1.1/CHANGELOG.md"
                            .to_string(),
                    ),
                }))
            });

        let mut extract = MockChangeExtractPort::new();
        extract
            .expect_extract_budget_exhausted()
            .returning(|| false);
        extract
            .expect_extract_change_items()
            .returning(|_| Ok(outcome(Vec::new())));

        let mut store = MockHistoryStorePort::new();
        store.expect_append_entry().returning(|_| Ok(()));

        // レジストリには古い（移動した）source が ai-discovered で保存されている。
        let mut registry = MockNotesSourceRegistryPort::new();
        registry.expect_read_registry().returning(|| {
            let mut r = NotesSourceRegistry::default();
            r.record(
                "openssl",
                DeltaSource::NixEval,
                NotesSourceEntry {
                    source: Some(
                        "https://github.com/openssl/openssl/blob/old/CHANGELOG".to_string(),
                    ),
                    origin: NotesOrigin::AiDiscovered,
                    discovered_at: None,
                    note: None,
                },
            );
            Ok(r)
        });
        // 自己修復で新ソース（origin=mechanical）へ更新する。
        registry
            .expect_write_registry()
            .times(1)
            .withf(|r| {
                r.lookup("openssl", DeltaSource::NixEval).is_some_and(|e| {
                    e.origin == NotesOrigin::Mechanical
                        // 表示用 notes_url ではなく refetch_url（raw changelog）を学習する（finding 3369076722）。
                        && e.source.as_deref()
                            == Some("https://raw.githubusercontent.com/openssl/openssl/v1.1/CHANGELOG.md")
                })
            })
            .returning(|_| Ok(()));

        run_record_with(
            command(),
            &nix_versions,
            &brew_diff,
            &notes,
            &extract,
            &store,
            &registry,
        )
    }

    #[test]
    fn temporary_all_path_failure_preserves_existing_saved_source() -> crate::Result<()> {
        // finding 3374863459 退行固定: 保存済みの有効 source があるパッケージで、その URL の fetch が一時的に失敗し
        // （reused=None）、同じ run の機械解決（mechanical=None）も AI 探索（source_url なし・空変更）も取得元を
        // 返せなかった場合、既存エントリを origin=none で **上書きしない**（有効な取得元を 1 回の不調で失わない）。
        // 退行版（else 分岐が無条件に origin=none を record）なら registry が書き換わり write_registry が呼ばれて
        // しまうため、ここでは **write_registry が一切呼ばれない**（registry を据え置く）ことで単一勝者性を固定する。
        let (nix_versions, brew_diff) = nix_only_diff("openssl", "openssl/openssl");

        let mut notes = MockNotesPort::new();
        // 保存 source の再利用 fetch が一時的に失敗（None）。
        notes
            .expect_fetch_notes_from_source()
            .times(1)
            .returning(|_| Ok(None));
        // 自己修復の機械解決も一時失敗で取得元を返せない（None）。
        notes
            .expect_fetch_release_notes()
            .times(1)
            .returning(|_, _, _, _, _, _| Ok(None));

        let mut extract = MockChangeExtractPort::new();
        extract
            .expect_extract_budget_exhausted()
            .returning(|| false);
        // AI も取得元を採用できない（source_url なし・空変更）。
        extract
            .expect_extract_change_items()
            .returning(|_| Ok(outcome(Vec::new())));

        let mut store = MockHistoryStorePort::new();
        store.expect_append_entry().returning(|_| Ok(()));

        // レジストリには有効な保存 source（mechanical・raw 再取得可能）が既に在る。
        let mut registry = MockNotesSourceRegistryPort::new();
        registry.expect_read_registry().returning(|| {
            let mut r = NotesSourceRegistry::default();
            r.record(
                "openssl",
                DeltaSource::NixEval,
                NotesSourceEntry {
                    source: Some(
                        "https://raw.githubusercontent.com/openssl/openssl/v1.0/CHANGELOG.md"
                            .to_string(),
                    ),
                    origin: NotesOrigin::Mechanical,
                    discovered_at: None,
                    note: None,
                },
            );
            Ok(r)
        });
        // 全経路一時失敗時は既存 source を保持し、registry を据え置く（write_registry を呼ばない）。
        registry.expect_write_registry().never();

        run_record_with(
            command(),
            &nix_versions,
            &brew_diff,
            &notes,
            &extract,
            &store,
            &registry,
        )
    }

    #[test]
    fn brew_cask_hint_routes_to_agent_loop_without_definition_seed() -> crate::Result<()> {
        // finding 3374863454 退行固定: brew delta は cask `.rb` 定義を seed にせず（summarize_only に入れない）、
        // 定義から取り出した探索ヒント（homepage/url）を `ExtractRequest.homepage` へ載せて agent_loop（seed なし
        // ＝tool-use 探索）へ回す。ここでは seed_notes が None のまま、解決した homepage ヒントが request に乗ることを固定する。
        let nix_versions = nix_versions_empty();
        let mut brew_diff = MockBrewVersionDiffPort::new();
        brew_diff
            .expect_diff_brew_versions()
            .returning(|_, _| Ok(vec![brew_delta("firefox")]));

        let mut notes = MockNotesPort::new();
        // cask `.rb` は seed にしない（None）。
        notes
            .expect_fetch_release_notes()
            .withf(|name, source, _, _, _, _| name == "firefox" && *source == DeltaSource::BrewTap)
            .times(1)
            .returning(|_, _, _, _, _, _| Ok(None));
        // seed 無し brew delta で探索ヒント（cask 定義の homepage）を解決する。
        notes
            .expect_resolve_brew_notes_hint()
            .withf(|name| name == "firefox")
            .times(1)
            .returning(|_| Ok(Some("https://www.mozilla.org/firefox/".to_string())));

        let mut extract = MockChangeExtractPort::new();
        extract
            .expect_extract_budget_exhausted()
            .returning(|| false);
        // ExtractRequest は seed 無し（agent_loop 経路）で、homepage に cask 探索ヒントが乗る。
        extract
            .expect_extract_change_items()
            .times(1)
            .withf(|request| {
                request.name == "firefox"
                    && request.seed_notes.is_none()
                    && request.homepage.as_deref() == Some("https://www.mozilla.org/firefox/")
            })
            .returning(|_| Ok(outcome(Vec::new())));

        let mut store = MockHistoryStorePort::new();
        store.expect_append_entry().returning(|_| Ok(()));

        run_record_with(
            command(),
            &nix_versions,
            &brew_diff,
            &notes,
            &extract,
            &store,
            &empty_registry_store(),
        )
    }

    #[test]
    fn disallowed_ai_source_is_not_learned() -> crate::Result<()> {
        // 退行固定（SSRF/学習境界）: AI が採用した取得元 URL が許可ホスト外なら、レジストリへ学習せず
        // origin=none へ倒す（次回フロー 1 で許可外 URL を fetch しない）。
        let (nix_versions, brew_diff) = nix_only_diff("neovim", "neovim/neovim");

        let mut notes = MockNotesPort::new();
        notes
            .expect_fetch_release_notes()
            .times(1)
            .returning(|_, _, _, _, _, _| Ok(None));

        let mut extract = MockChangeExtractPort::new();
        extract
            .expect_extract_budget_exhausted()
            .returning(|| false);
        // AI が許可外 host の取得元 URL を返す（本来 SSRF 検査を通った fetch のはずだが、二重防御として記録側でも弾く）。
        extract
            .expect_extract_change_items()
            .times(1)
            .returning(|_| {
                Ok(ExtractOutcome {
                    items: Vec::new(),
                    source_url: Some("https://evil.example/notes".to_string()),
                })
            });

        let mut store = MockHistoryStorePort::new();
        store.expect_append_entry().returning(|_| Ok(()));

        let mut registry = MockNotesSourceRegistryPort::new();
        registry
            .expect_read_registry()
            .returning(|| Ok(NotesSourceRegistry::default()));
        // 許可外 source は学習しない: origin=none・source なしで記録される。
        registry
            .expect_write_registry()
            .times(1)
            .withf(|r| {
                r.lookup("neovim", DeltaSource::NixEval)
                    .is_some_and(|e| e.origin == NotesOrigin::None && e.source.is_none())
            })
            .returning(|_| Ok(()));

        run_record_with(
            command(),
            &nix_versions,
            &brew_diff,
            &notes,
            &extract,
            &store,
            &registry,
        )
    }

    #[test]
    fn registry_lookup_is_keyed_by_source_not_just_name() -> crate::Result<()> {
        // 退行固定（finding 3369076719）: 同名 `firefox` の nix delta と brew delta があるとき、レジストリ参照は
        // 出所込みキーで引く。レジストリには brew/firefox の保存 source だけがあり、nix/firefox は未登録とする。
        // 旧実装（name だけのキー）なら nix delta が brew の保存 source を誤って再利用してしまうが、出所込みキー
        // では nix/firefox は未 hit → 機械解決へ倒れる（別出所の取得元を取り違えない）。
        let mut nix = MockNixVersionPort::new();
        nix.expect_old_versions().returning(|| {
            Ok(BTreeMap::from([(
                "firefox".to_string(),
                pkg("120", "", ""),
            )]))
        });
        nix.expect_new_versions().returning(|| {
            Ok(BTreeMap::from([(
                "firefox".to_string(),
                pkg("121", "mozilla/firefox", ""),
            )]))
        });
        let mut brew_diff = MockBrewVersionDiffPort::new();
        brew_diff
            .expect_diff_brew_versions()
            .returning(|_, _| Ok(vec![brew_delta("firefox")]));

        let mut notes = MockNotesPort::new();
        // brew/firefox は保存 source を再利用して fetch（fetch_notes_from_source）。
        notes
            .expect_fetch_notes_from_source()
            .withf(|url| url == "https://github.com/homebrew/homebrew-cask/blob/x/firefox.rb")
            .times(1)
            .returning(|url| {
                Ok(Some(RawReleaseNotes {
                    text: "brew reused".to_string(),
                    notes_url: url.to_string(),
                    refetch_url: Some(url.to_string()),
                }))
            });
        // nix/firefox は未 hit のため機械解決を試みる（別出所の brew source を再利用しない）。
        notes
            .expect_fetch_release_notes()
            .withf(|name, source, _, _, _, _| name == "firefox" && *source == DeltaSource::NixEval)
            .times(1)
            .returning(|_, _, _, _, _, _| Ok(None));

        let mut extract = MockChangeExtractPort::new();
        extract
            .expect_extract_budget_exhausted()
            .returning(|| false);
        extract
            .expect_extract_change_items()
            .returning(|_| Ok(outcome(Vec::new())));

        let mut store = MockHistoryStorePort::new();
        store
            .expect_append_entry()
            .times(1)
            .withf(|entry| entry.packages.len() == 2)
            .returning(|_| Ok(()));

        // レジストリには brew/firefox だけが登録済み（nix/firefox は未登録）。
        let mut registry = MockNotesSourceRegistryPort::new();
        registry.expect_read_registry().returning(|| {
            let mut r = NotesSourceRegistry::default();
            r.record(
                "firefox",
                DeltaSource::BrewTap,
                NotesSourceEntry {
                    source: Some(
                        "https://github.com/homebrew/homebrew-cask/blob/x/firefox.rb".to_string(),
                    ),
                    origin: NotesOrigin::AiDiscovered,
                    discovered_at: None,
                    note: None,
                },
            );
            Ok(r)
        });
        // nix/firefox の機械解決は None なので学習は origin=none（write は走る）。
        registry.expect_write_registry().returning(|_| Ok(()));

        run_record_with(
            command(),
            &nix,
            &brew_diff,
            &notes,
            &extract,
            &store,
            &registry,
        )
    }
}
