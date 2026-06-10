//! record use case: 版差分を集め、ノートを取得・LLM 抽出して履歴へ 1 エントリ追記する（1 回で完結）。
//!
//! ## record
//!
//! nix/brew の版差分を統合し、各アプリのノートを **レジストリ参照 → 機械解決 → AI 探索**の順で取得・LLM 抽出
//! して 1 エントリを履歴へ追記し、取得元（provenance）をレジストリへ学習する。ノートが取れない/抽出が空の
//! パッケージはその場で **version-only**（version old→new + notes_url のみ、change_items 空）として確定記録する
//! （夜をまたいで再試行しない）。
//!
//! ## provenance（利用者要件 (3)/(4)）
//!
//! 各パッケージごとに [registry 参照 → 機械解決 → AI 探索] でノートを得て、取得元 URL + origin をレジストリへ
//! 学習し次回再利用する（再探索しない＝レート逓減）。レジストリへ書く URL は記録前に host allowlist で検証する。

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::diff::{DeltaSource, NixPackage, VersionDelta, diff_versions, merge_version_deltas};
use super::llm::{ChangeExtractor, ExtractRequest, OpenAiExtractor};
use super::notes::{self, RawReleaseNotes};
use super::wire::{
    ChangeItem, PackageSource, PackageUpdate, UpdateEntry, is_allowed_url, overall_headline,
    sanitize_change_items, sanitize_notes_url, severity_of,
};
use super::{brew, eval};
use crate::Result;

/// record use case の入力（記録する rev・時刻・参照と版差分入力）。
pub(crate) struct RecordInput<'a> {
    /// bump 前 nixpkgs リビジョン。
    pub(crate) nixpkgs_old: String,
    /// bump 後 nixpkgs リビジョン。
    pub(crate) nixpkgs_new: String,
    /// diff 対象の参照構成（例: `darwinConfigurations.<ref>`）。
    pub(crate) reference: String,
    /// 適用時刻（RFC3339。CI が注入する）。
    pub(crate) at: String,
    /// 追記先の月次 TOML ファイル。
    pub(crate) out: &'a Path,
    /// provenance レジストリ TOML ファイル。
    pub(crate) registry_path: &'a Path,
    /// bump 前 lock の eval JSON ファイル（無ければ nix old は空）。
    pub(crate) nix_old: Option<&'a Path>,
    /// bump 後 lock の eval JSON ファイル（無ければ `reference` を `nix eval` して導出する）。
    pub(crate) nix_new: Option<&'a Path>,
    /// 宣言 cask を読む `homebrew.nix` path（cask rev と対で brew 版差分を算出する）。
    pub(crate) homebrew_nix: Option<&'a Path>,
    /// brew cask tap の bump 前 rev（cask 版差分に使う）。
    pub(crate) cask_rev_old: Option<&'a str>,
    /// brew cask tap の bump 後 rev（cask 版差分に使う）。
    pub(crate) cask_rev_new: Option<&'a str>,
}

/// 1 パッケージ分の素材（version 差分 + 変更リスト + ノート URL）。change_items が空なら version-only。
struct PackageMaterial {
    delta: VersionDelta,
    change_items: Vec<ChangeItem>,
    notes_url: Option<String>,
}

/// version 差分 1 件を記録用 [`PackageUpdate`] へ変換する（nix/brew いずれも宣言アプリ＝declared=true）。
fn to_package_update(material: PackageMaterial) -> PackageUpdate {
    let source = match material.delta.source {
        DeltaSource::NixEval => PackageSource::Nix,
        DeltaSource::BrewTap => PackageSource::Brew,
    };
    PackageUpdate {
        name: material.delta.name,
        old: material.delta.old,
        new: material.delta.new,
        change: material.delta.change,
        declared: true,
        source,
        notes_url: material.notes_url,
        change_items: material.change_items,
    }
}

/// パッケージ素材列から、severity / overall を機械算出した 1 件の [`UpdateEntry`] を組み立てる。
fn build_entry(
    at: String,
    nixpkgs_old: String,
    nixpkgs_new: String,
    reference: String,
    materials: Vec<PackageMaterial>,
) -> UpdateEntry {
    let packages: Vec<PackageUpdate> = materials.into_iter().map(to_package_update).collect();
    let all_items: Vec<ChangeItem> = packages
        .iter()
        .flat_map(|package| package.change_items.clone())
        .collect();
    let severity = severity_of(&all_items);
    let overall = overall_headline(packages.len(), &all_items);
    UpdateEntry {
        at,
        nixpkgs_old,
        nixpkgs_new,
        reference,
        severity,
        overall,
        packages,
    }
}

/// レジストリ参照（フロー 1）で保存済み source からノートを再取得する HTTP seam。
///
/// 本番経路は [`notes::fetch_from_source`]（reqwest）に解決し、テストは network 不要な fake へ差し替える。
/// host allowlist 検査は呼び出し側で済むため、この seam は与えられた URL をそのまま取得する契約。
type NotesFetch<'a> = dyn Fn(&str) -> Result<Option<RawReleaseNotes>> + 'a;

/// [`resolve_notes`] の結果（change_items + ノート URL）。change_items が空なら version-only。
struct ResolvedNotes {
    change_items: Vec<ChangeItem>,
    notes_url: Option<String>,
}

/// 1 delta のノートを [registry 参照 → 機械解決 → AI 探索] で得て、change_items と記録 URL を確定する。
///
/// `extract` は LLM seam、`fetch_source` は registry 再利用フローの HTTP seam（注入差し替え可）。
/// `registry`/`registry_dirty` は provenance 学習で更新する。change_items が空なら呼び出し側が version-only と
/// して記録する。
fn resolve_notes(
    delta: &VersionDelta,
    at: &str,
    extract: &dyn ChangeExtractor,
    fetch_source: &NotesFetch<'_>,
    brew_hint: &dyn Fn(&str) -> Result<Option<String>>,
    registry: &mut NotesSourceRegistry,
    registry_dirty: &mut bool,
) -> Result<ResolvedNotes> {
    // フロー 1（レジストリ参照）: 保存済み有効 source を直接 fetch して再利用（再探索しない）。
    let saved_source = registry
        .lookup(&delta.name, delta.source)
        .and_then(NotesSourceEntry::reusable_source)
        .filter(|url| is_allowed_url(url))
        .map(str::to_string);
    let reused = match saved_source.as_deref() {
        Some(url) => fetch_source(url)?.map(|notes| (url.to_string(), notes)),
        None => None,
    };

    // フロー 2（機械解決）: 未登録 or 自己修復なら Releases API / changelog で取得する。
    let mechanical = match &reused {
        Some(_) => None,
        None => notes::fetch_release_notes(delta)?,
    };

    let (seed, resolved_notes_url): (Option<RawReleaseNotes>, Option<String>) =
        match (&reused, &mechanical) {
            (Some((url, notes_text)), _) => (Some(notes_text.clone()), Some(url.clone())),
            (None, Some(notes_text)) => {
                (Some(notes_text.clone()), Some(notes_text.notes_url.clone()))
            }
            (None, None) => (None, None),
        };

    // brew cask の探索ヒント（seed が無い brew delta のみ）。
    let brew_homepage_hint = if delta.source == DeltaSource::BrewTap && seed.is_none() {
        brew_hint(&delta.name)?
    } else {
        None
    };

    // 単一の AI 抽出（失敗・キー未設定は extract 内で空へ縮退し、呼び出し側が version-only として記録する）。
    let request = ExtractRequest::from_delta(delta, seed, brew_homepage_hint);
    let outcome = extract.extract(&request)?;
    let change_items = sanitize_change_items(outcome.items);

    // provenance を確定して学習する（フロー別）。
    if reused.is_some() {
        // 据え置き（再探索しない）。
    } else if let Some(mech) = &mechanical {
        let provenance = match &mech.refetch_url {
            Some(refetch_url) => NotesSourceEntry {
                source: Some(refetch_url.clone()),
                origin: NotesOrigin::Mechanical,
                discovered_at: Some(at.to_string()),
                note: None,
            },
            None => NotesSourceEntry {
                source: None,
                origin: NotesOrigin::None,
                discovered_at: Some(at.to_string()),
                note: None,
            },
        };
        learn_provenance(
            registry,
            registry_dirty,
            &delta.name,
            delta.source,
            provenance,
        );
    } else if let Some(source_url) = outcome
        .source_url
        .as_ref()
        .filter(|_| !change_items.is_empty())
    {
        let provenance = NotesSourceEntry {
            source: Some(source_url.clone()),
            origin: NotesOrigin::AiDiscovered,
            discovered_at: Some(at.to_string()),
            note: None,
        };
        learn_provenance(
            registry,
            registry_dirty,
            &delta.name,
            delta.source,
            provenance,
        );
    } else if registry
        .lookup(&delta.name, delta.source)
        .and_then(NotesSourceEntry::reusable_source)
        .is_some()
    {
        // 全経路が空を返したが既存有効 source が在る一時失敗 → 既存を保持（上書きしない）。
    } else {
        let provenance = NotesSourceEntry {
            source: None,
            origin: NotesOrigin::None,
            discovered_at: Some(at.to_string()),
            note: None,
        };
        learn_provenance(
            registry,
            registry_dirty,
            &delta.name,
            delta.source,
            provenance,
        );
    }

    let notes_url = sanitize_notes_url(
        resolved_notes_url
            .or_else(|| outcome.source_url.clone())
            .or_else(|| delta.notes_source.clone())
            .or_else(|| delta.homepage.clone()),
    );
    Ok(ResolvedNotes {
        change_items,
        notes_url,
    })
}

/// 確定した provenance をサニタイズしてレジストリへ学習し、更新フラグを立てる。
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
fn sanitize_provenance(entry: NotesSourceEntry) -> NotesSourceEntry {
    match entry.source {
        Some(ref url) if is_allowed_url(url) => entry,
        Some(_) => NotesSourceEntry {
            source: None,
            origin: NotesOrigin::None,
            discovered_at: entry.discovered_at,
            note: entry.note,
        },
        None => entry,
    }
}

/// record use case: 版差分 → ノート取得・抽出 → 履歴追記（取れないものは version-only 確定）。provenance を学習する。
///
/// bump 後 nix 版は `--nix-new` ファイルがあればそれを、無ければ `reference` を `nix eval` して Rust で導出する。
/// brew cask 版差分は `homebrew.nix` + 両 cask rev から reqwest で算出する。
pub(crate) fn run_record(input: RecordInput<'_>, extract: &OpenAiExtractor) -> Result<()> {
    run_record_with(
        &input,
        extract,
        &notes::fetch_from_source,
        &|name| extract.brew_homepage_hint(name),
        &|reference| eval::eval_declared_versions(reference),
        &brew::fetch_cask_rb,
    )
}

/// テスト可能な record 本体（LLM seam・registry 再利用 fetch seam・brew ヒント・nix eval・cask fetch を注入する）。
fn run_record_with(
    input: &RecordInput<'_>,
    extract: &dyn ChangeExtractor,
    fetch_source: &NotesFetch<'_>,
    brew_hint: &dyn Fn(&str) -> Result<Option<String>>,
    eval_new: &dyn Fn(&str) -> Result<std::collections::BTreeMap<String, NixPackage>>,
    fetch_cask: &dyn Fn(&str) -> Result<Option<String>>,
) -> Result<()> {
    let old_versions = notes::read_nix_versions(input.nix_old)?;
    let new_versions = match input.nix_new {
        Some(path) => notes::read_nix_versions(Some(path))?,
        None => eval_new(&input.reference)?,
    };
    let nix_deltas = diff_versions(&old_versions, &new_versions);
    let brew_deltas = compute_brew_deltas(input, fetch_cask)?;
    let deltas = merge_version_deltas(nix_deltas, brew_deltas);

    let mut registry = read_registry(input.registry_path)?;
    let mut registry_dirty = false;

    let mut materials = Vec::with_capacity(deltas.len());
    let mut summarized = 0usize;
    let mut version_only = 0usize;
    for delta in deltas {
        let resolved = resolve_notes(
            &delta,
            &input.at,
            extract,
            fetch_source,
            brew_hint,
            &mut registry,
            &mut registry_dirty,
        )?;
        // ノートが取れなければ（change_items 空）その場で version-only として確定する（夜をまたいで再試行しない）。
        if resolved.change_items.is_empty() {
            version_only += 1;
        } else {
            summarized += 1;
        }
        materials.push(PackageMaterial {
            delta,
            change_items: resolved.change_items,
            notes_url: resolved.notes_url,
        });
    }
    if summarized + version_only > 0 {
        eprintln!("notes: {summarized} packages summarized, {version_only} version-only");
    }

    if registry_dirty {
        write_registry(input.registry_path, &registry)?;
    }

    // rev 前進なし・差分素材なしの夜は chain link にも要約にもならない空エントリを残さない。
    let rev_advanced = input.nixpkgs_old != input.nixpkgs_new;
    if !rev_advanced && materials.is_empty() {
        return Ok(());
    }

    let entry = build_entry(
        input.at.clone(),
        input.nixpkgs_old.clone(),
        input.nixpkgs_new.clone(),
        input.reference.clone(),
        materials,
    );
    append_entry(input.out, &entry)
}

/// `homebrew.nix` + 両 cask rev が揃うときだけ、reqwest で cask `.rb` を取得して brew 版差分を算出する。
///
/// いずれかが欠ける（cask 差分不要 / テスト）なら空。cask list と version 解析・auto_updates 除外は [`brew`]。
fn compute_brew_deltas(
    input: &RecordInput<'_>,
    fetch_cask: &dyn Fn(&str) -> Result<Option<String>>,
) -> Result<Vec<VersionDelta>> {
    let (Some(homebrew_nix), Some(rev_old), Some(rev_new)) =
        (input.homebrew_nix, input.cask_rev_old, input.cask_rev_new)
    else {
        return Ok(Vec::new());
    };
    let casks_nix = std::fs::read_to_string(homebrew_nix)?;
    brew::diff_casks(&casks_nix, rev_old, rev_new, fetch_cask)
}

// ---- provenance レジストリ（学習・再利用の wire/ドメイン型と決定論規則） ----
//
// どこからノートを取得したか（provenance）を repo 管理の TOML（`docs/update-history/notes-sources.toml`）へ
// 保存し、次回以降は参照して再利用し再探索しない。AI-discovered で書く `source` URL は AI 由来であり、記録前に
// host allowlist（[`is_allowed_url`]）で機械検証して許可外を学習しない（`origin=none` へ倒す）。

/// ノート取得元の出所（どの解決経路で取得元が確定したか）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum NotesOrigin {
    /// 機械解決（Releases API range / changelog 解決）で取得元 URL が確定した。
    Mechanical,
    /// AI エージェント探索が実際に fetch して有効ノートを得た取得元 URL が確定した。
    AiDiscovered,
    /// 有効な取得元が見つからなかった（version-only へ縮退）。次回も探索対象に戻す。
    None,
}

/// レジストリ 1 エントリ（1 パッケージの provenance）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct NotesSourceEntry {
    /// 実際にノートを取得した URL（許可ホスト https。`origin=none` では `None`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<String>,
    /// 取得元の出所（再探索要否の判断材料）。
    pub(crate) origin: NotesOrigin,
    /// 記録時刻（任意・人間可読の RFC3339）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) discovered_at: Option<String>,
    /// 任意の人間可読メモ。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) note: Option<String>,
}

impl NotesSourceEntry {
    /// レジストリ参照（フロー 1）で再利用できる有効な保存 source を返す（`origin=none`/source 不在は `None`）。
    pub(crate) fn reusable_source(&self) -> Option<&str> {
        match self.origin {
            NotesOrigin::Mechanical | NotesOrigin::AiDiscovered => self.source.as_deref(),
            NotesOrigin::None => None,
        }
    }
}

/// パッケージ名 → 取得元エントリのレジストリ（決定論・安定ソートの map）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct NotesSourceRegistry {
    entries: BTreeMap<String, NotesSourceEntry>,
}

/// パッケージ名と出所からレジストリの一意キー `<source>/<name>` を組み立てる純粋関数。
fn registry_key(name: &str, source: DeltaSource) -> String {
    format!("{}/{name}", source.as_stable_key())
}

impl NotesSourceRegistry {
    /// 指定パッケージ（名前 + 出所）の保存済みエントリを参照する（無ければ `None`）。
    pub(crate) fn lookup(&self, name: &str, source: DeltaSource) -> Option<&NotesSourceEntry> {
        self.entries.get(&registry_key(name, source))
    }

    /// 指定パッケージ（名前 + 出所）の取得元を記録（追記/上書き）する。
    pub(crate) fn record(&mut self, name: &str, source: DeltaSource, entry: NotesSourceEntry) {
        self.entries.insert(registry_key(name, source), entry);
    }
}

// ---- 履歴 TOML / レジストリのファイル I/O ----
//
// 履歴は 1 ファイルに複数の `[[update]]` を持つ。show は月次ファイルが並ぶ directory を連結読みし、record は
// 単一の `<YYYY-MM>.toml` を read/append する。registry は決定論（名前昇順）で read/write する。

/// 履歴 TOML ファイル全体の wire 表現（`[[update]]` の列）。
#[derive(Default, Serialize, Deserialize)]
struct HistoryDocument {
    #[serde(default, rename = "update")]
    updates: Vec<UpdateEntry>,
}

/// ファイル名が月次履歴ファイル `<YYYY-MM>.toml` 形かを判定する純粋関数（registry/非履歴 TOML を除外する）。
fn is_monthly_history_file(file_name: &str) -> bool {
    let Some(stem) = file_name.strip_suffix(".toml") else {
        return false;
    };
    let Some((year, month)) = stem.split_once('-') else {
        return false;
    };
    year.len() == 4
        && month.len() == 2
        && year.bytes().all(|b| b.is_ascii_digit())
        && month.bytes().all(|b| b.is_ascii_digit())
}

/// 単一履歴ファイルを読み、document を返す（不存在なら空 document）。
fn read_document(path: &Path) -> Result<HistoryDocument> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(toml::from_str(&text)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(HistoryDocument::default())
        }
        Err(error) => Err(error.into()),
    }
}

/// directory 配下の月次履歴ファイルだけを名前順に読み、エントリを連結する（registry/非月次を除外）。
fn read_directory(path: &Path) -> Result<Vec<UpdateEntry>> {
    let mut files: Vec<std::path::PathBuf> = match std::fs::read_dir(path) {
        Ok(read_dir) => read_dir
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(is_monthly_history_file)
            })
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    files.sort();
    let mut entries = Vec::new();
    for file in files {
        entries.extend(read_document(&file)?.updates);
    }
    Ok(entries)
}

/// 履歴 source（ファイル/ディレクトリ）の全エントリを読む。directory なら月次ファイルを名前順に連結する。
pub(crate) fn read_entries(source: &Path) -> Result<Vec<UpdateEntry>> {
    if source.is_dir() {
        read_directory(source)
    } else {
        Ok(read_document(source)?.updates)
    }
}

/// 親 directory を確保し、document を TOML で書き戻す。
fn write_document(path: &Path, document: &HistoryDocument) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, toml::to_string(document)?)?;
    Ok(())
}

/// 新エントリを既存履歴へ追記する（既存エントリは保持する）。
pub(crate) fn append_entry(path: &Path, entry: &UpdateEntry) -> Result<()> {
    let mut document = read_document(path)?;
    document.updates.push(entry.clone());
    write_document(path, &document)
}

/// provenance レジストリを読む（不存在なら空レジストリ）。
fn read_registry(path: &Path) -> Result<NotesSourceRegistry> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(toml::from_str(&text)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(NotesSourceRegistry::default())
        }
        Err(error) => Err(error.into()),
    }
}

/// provenance レジストリ全体を決定論（名前昇順は domain の BTreeMap が保証）で書き戻す。
fn write_registry(path: &Path, registry: &NotesSourceRegistry) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, toml::to_string(registry)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    //! record の差分→change_items 付き TOML・version-only 確定・provenance 学習・再利用を、LLM/HTTP fake seam で
    //! 決定論的に固定する。

    use super::*;
    use crate::update_history::diff::DeltaSource;
    use crate::update_history::llm::{ChangeExtractor, ExtractOutcome, ExtractRequest};
    use crate::update_history::wire::{ChangeCategory, ChangeItem};
    use std::cell::RefCell;
    use std::path::PathBuf;

    /// fake LLM extractor: 各パッケージ名に対する抽出結果を返し、呼び出しを記録する。
    struct FakeExtractor {
        /// 名前 → 返す outcome。未登録名は空（version-only）。
        responses: RefCell<std::collections::BTreeMap<String, ExtractOutcome>>,
        /// extract 呼び出し回数。
        calls: RefCell<u32>,
    }

    impl FakeExtractor {
        fn new() -> Self {
            Self {
                responses: RefCell::new(std::collections::BTreeMap::new()),
                calls: RefCell::new(0),
            }
        }
        fn with(name: &str, outcome: ExtractOutcome) -> Self {
            let f = Self::new();
            f.responses.borrow_mut().insert(name.to_string(), outcome);
            f
        }
    }

    impl ChangeExtractor for FakeExtractor {
        fn extract(&self, request: &ExtractRequest) -> Result<ExtractOutcome> {
            *self.calls.borrow_mut() += 1;
            Ok(self
                .responses
                .borrow()
                .get(&request.name)
                .cloned()
                .unwrap_or_default())
        }
    }

    fn no_brew_hint(_: &str) -> Result<Option<String>> {
        Ok(None)
    }

    /// registry 再利用 fetch を踏まない seam（保存済み source が無い経路の固定。空）。
    fn no_fetch_source(_: &str) -> Result<Option<RawReleaseNotes>> {
        Ok(None)
    }

    /// nix-new ファイルを与えるテストでは eval seam を踏まない（呼ばれたら空マップ）。
    fn no_eval(_: &str) -> Result<std::collections::BTreeMap<String, NixPackage>> {
        Ok(std::collections::BTreeMap::new())
    }

    /// cask 差分を踏まないテスト seam（homebrew_nix 未指定なら呼ばれない）。
    fn no_cask(_: &str) -> Result<Option<String>> {
        Ok(None)
    }

    fn outcome(items: Vec<ChangeItem>) -> ExtractOutcome {
        ExtractOutcome {
            items,
            source_url: None,
        }
    }

    fn item(category: ChangeCategory, text: &str) -> ChangeItem {
        ChangeItem {
            category,
            text: text.to_string(),
            ref_url: None,
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("dotfiles-uh-record-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok();
        dir
    }

    fn write_nix(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, content).ok();
        path
    }

    fn input<'a>(
        dir: &'a Path,
        out: &'a Path,
        registry: &'a Path,
        nix_old: Option<&'a Path>,
        nix_new: Option<&'a Path>,
    ) -> RecordInput<'a> {
        let _ = dir;
        RecordInput {
            nixpkgs_old: "a1b2c3d".to_string(),
            nixpkgs_new: "e4f5g6h".to_string(),
            reference: "darwinConfigurations.ci".to_string(),
            at: "2026-06-05T18:00:11Z".to_string(),
            out,
            registry_path: registry,
            nix_old,
            nix_new,
            homebrew_nix: None,
            cask_rev_old: None,
            cask_rev_new: None,
        }
    }

    #[test]
    fn record_writes_change_items_for_summarized_package() -> Result<()> {
        let dir = temp_dir("summarized");
        let old = write_nix(
            &dir,
            "old.json",
            r#"{"neovim":{"version":"0.10","repo":"neovim/neovim"}}"#,
        );
        // repo は機械解決を試みるが network 不要にするため、抽出が seed 無しでも outcome を返す fake。
        let new = write_nix(&dir, "new.json", r#"{"neovim":{"version":"0.11"}}"#);
        let out = dir.join("2026-06.toml");
        let registry = dir.join("notes-sources.toml");
        let extract = FakeExtractor::with(
            "neovim",
            outcome(vec![item(ChangeCategory::Feature, "新機能")]),
        );

        run_record_with(
            &input(&dir, &out, &registry, Some(&old), Some(&new)),
            &extract,
            &no_fetch_source,
            &no_brew_hint,
            &no_eval,
            &no_cask,
        )?;

        let entries = read_entries(&out)?;
        assert_eq!(entries.len(), 1);
        let pkg = &entries[0].packages[0];
        assert_eq!(pkg.name, "neovim");
        assert_eq!(pkg.change_items.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn record_records_version_only_when_no_notes() -> Result<()> {
        let dir = temp_dir("version-only");
        let old = write_nix(&dir, "old.json", r#"{"obscure":{"version":"1.0"}}"#);
        let new = write_nix(&dir, "new.json", r#"{"obscure":{"version":"1.1"}}"#);
        let out = dir.join("2026-06.toml");
        let registry = dir.join("notes-sources.toml");
        // fake は obscure に何も返さない → その場で version-only として確定（change_items 空・version 保持）。
        let extract = FakeExtractor::new();

        run_record_with(
            &input(&dir, &out, &registry, Some(&old), Some(&new)),
            &extract,
            &no_fetch_source,
            &no_brew_hint,
            &no_eval,
            &no_cask,
        )?;

        let entries = read_entries(&out)?;
        let pkg = &entries[0].packages[0];
        assert!(pkg.change_items.is_empty(), "概要未取得は change_items 空");
        assert_eq!(pkg.old.as_deref(), Some("1.0"));
        assert_eq!(pkg.new.as_deref(), Some("1.1"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn record_learns_ai_discovered_provenance() -> Result<()> {
        let dir = temp_dir("provenance");
        let old = write_nix(&dir, "old.json", r#"{"neovim":{"version":"0.10"}}"#);
        let new = write_nix(&dir, "new.json", r#"{"neovim":{"version":"0.11"}}"#);
        let out = dir.join("2026-06.toml");
        let registry = dir.join("notes-sources.toml");
        // AI が取得元 URL を採用し、有効な change_items を返す → ai-discovered 学習。
        let extract = FakeExtractor::with(
            "neovim",
            ExtractOutcome {
                items: vec![item(ChangeCategory::Feature, "新機能")],
                source_url: Some("https://github.com/neovim/neovim/releases".to_string()),
            },
        );

        run_record_with(
            &input(&dir, &out, &registry, Some(&old), Some(&new)),
            &extract,
            &no_fetch_source,
            &no_brew_hint,
            &no_eval,
            &no_cask,
        )?;

        let learned = read_registry(&registry)?;
        let entry = learned.lookup("neovim", DeltaSource::NixEval);
        assert!(entry.is_some_and(|e| e.origin == NotesOrigin::AiDiscovered
            && e.source.as_deref() == Some("https://github.com/neovim/neovim/releases")));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn record_does_not_learn_ai_source_when_change_items_empty() -> Result<()> {
        let dir = temp_dir("provenance-empty");
        let old = write_nix(&dir, "old.json", r#"{"neovim":{"version":"0.10"}}"#);
        let new = write_nix(&dir, "new.json", r#"{"neovim":{"version":"0.11"}}"#);
        let out = dir.join("2026-06.toml");
        let registry = dir.join("notes-sources.toml");
        // source_url は採用したが change_items が空 → ai-discovered 学習しない（origin=none）。
        let extract = FakeExtractor::with(
            "neovim",
            ExtractOutcome {
                items: Vec::new(),
                source_url: Some("https://github.com/neovim/neovim".to_string()),
            },
        );
        run_record_with(
            &input(&dir, &out, &registry, Some(&old), Some(&new)),
            &extract,
            &no_fetch_source,
            &no_brew_hint,
            &no_eval,
            &no_cask,
        )?;
        let learned = read_registry(&registry)?;
        let entry = learned.lookup("neovim", DeltaSource::NixEval);
        assert!(entry.is_some_and(|e| e.origin == NotesOrigin::None && e.source.is_none()));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn record_appends_empty_chain_link_when_rev_advanced_but_no_deltas() -> Result<()> {
        let dir = temp_dir("chain-link");
        let out = dir.join("2026-06.toml");
        let registry = dir.join("notes-sources.toml");
        // nix old/new 無し → 差分なし。rev 前進あり（a1b2c3d != e4f5g6h）→ 空 chain link を追記する。
        let extract = FakeExtractor::new();
        run_record_with(
            &input(&dir, &out, &registry, None, None),
            &extract,
            &no_fetch_source,
            &no_brew_hint,
            &no_eval,
            &no_cask,
        )?;
        let entries = read_entries(&out)?;
        assert_eq!(entries.len(), 1);
        assert!(entries[0].packages.is_empty());
        assert_eq!(entries[0].overall, "0アプリ更新");
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn record_skips_append_when_rev_unchanged_and_empty() -> Result<()> {
        let dir = temp_dir("skip-empty");
        let out = dir.join("2026-06.toml");
        let registry = dir.join("notes-sources.toml");
        let mut inp = input(&dir, &out, &registry, None, None);
        inp.nixpkgs_new = inp.nixpkgs_old.clone();
        let extract = FakeExtractor::new();
        run_record_with(
            &inp,
            &extract,
            &no_fetch_source,
            &no_brew_hint,
            &no_eval,
            &no_cask,
        )?;
        // rev 不変・空素材 → append しない（ファイルが存在しない）。
        assert!(read_entries(&out)?.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn registry_reuse_skips_re_extraction_path() -> Result<()> {
        // 退行固定（再利用フロー 1）: 保存済み有効 source があれば fetch_from_source を試みる。host allowlist で
        // 許可された URL のみ再利用するが、network 不要にするため許可外 host を保存して reused=None に倒し、
        // 機械解決（nix repo 無し）も空 → version-only になることを確認する（再利用判定の経路を通す）。
        let dir = temp_dir("reuse");
        let old = write_nix(&dir, "old.json", r#"{"neovim":{"version":"0.10"}}"#);
        let new = write_nix(&dir, "new.json", r#"{"neovim":{"version":"0.11"}}"#);
        let out = dir.join("2026-06.toml");
        let registry_path = dir.join("notes-sources.toml");
        let mut registry = NotesSourceRegistry::default();
        // 許可外 host は再利用 fetch を踏まない（is_allowed_url で除外）→ 機械解決へ。
        registry.record(
            "neovim",
            DeltaSource::NixEval,
            NotesSourceEntry {
                source: Some("https://evil.example/notes".to_string()),
                origin: NotesOrigin::Mechanical,
                discovered_at: None,
                note: None,
            },
        );
        write_registry(&registry_path, &registry)?;
        let extract = FakeExtractor::new();
        run_record_with(
            &input(&dir, &out, &registry_path, Some(&old), Some(&new)),
            &extract,
            &no_fetch_source,
            &no_brew_hint,
            &no_eval,
            &no_cask,
        )?;
        let entries = read_entries(&out)?;
        assert!(entries[0].packages[0].change_items.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn registry_reuse_success_skips_mechanical_and_keeps_existing_provenance() -> Result<()> {
        // 正常路径（再利用フロー 1 成功）: 保存済み許可 source を fetch_source seam が取得成功 → 機械解決を踏まず
        // （fetch_source 1 回のみ呼ばれる）、seed を根拠に抽出が概要を返し complete 記録。かつ既存 provenance
        // （source / origin / discovered_at）を上書きしないことを byte 等価で固定する（network 不要の決定論）。
        let dir = temp_dir("reuse-success");
        let old = write_nix(&dir, "old.json", r#"{"neovim":{"version":"0.10"}}"#);
        let new = write_nix(&dir, "new.json", r#"{"neovim":{"version":"0.11"}}"#);
        let out = dir.join("2026-06.toml");
        let registry_path = dir.join("notes-sources.toml");
        let saved_url = "https://github.com/neovim/neovim/releases";
        let mut registry = NotesSourceRegistry::default();
        let saved_entry = NotesSourceEntry {
            source: Some(saved_url.to_string()),
            origin: NotesOrigin::Mechanical,
            discovered_at: Some("2026-05-01T00:00:00Z".to_string()),
            note: None,
        };
        registry.record("neovim", DeltaSource::NixEval, saved_entry.clone());
        write_registry(&registry_path, &registry)?;

        // fetch_source seam: 保存済み URL のみ有効ノートを返し、呼び出し回数を観測する。
        let fetch_calls = RefCell::new(0u32);
        let fetched_urls = RefCell::new(Vec::<String>::new());
        let fetch_source = |url: &str| -> Result<Option<RawReleaseNotes>> {
            *fetch_calls.borrow_mut() += 1;
            fetched_urls.borrow_mut().push(url.to_string());
            Ok(if url == saved_url {
                Some(RawReleaseNotes {
                    text: "0.11 release notes".to_string(),
                    notes_url: url.to_string(),
                    refetch_url: Some(url.to_string()),
                })
            } else {
                None
            })
        };
        // seed を根拠に抽出が概要を返す（complete 記録になる）。
        let extract = FakeExtractor::with(
            "neovim",
            outcome(vec![item(ChangeCategory::Feature, "新機能")]),
        );

        run_record_with(
            &input(&dir, &out, &registry_path, Some(&old), Some(&new)),
            &extract,
            &fetch_source,
            &no_brew_hint,
            &no_eval,
            &no_cask,
        )?;

        // 再利用 fetch は保存済み URL に対し 1 回だけ（機械解決の追加 fetch は踏まない）。
        assert_eq!(*fetch_calls.borrow(), 1);
        assert_eq!(fetched_urls.borrow().as_slice(), [saved_url.to_string()]);

        // 概要付きとして記録され、再利用 source を notes_url に採る。
        let entries = read_entries(&out)?;
        let pkg = &entries[0].packages[0];
        assert_eq!(pkg.change_items.len(), 1);
        assert_eq!(pkg.notes_url.as_deref(), Some(saved_url));

        // 既存 provenance は上書きされない（source / origin / discovered_at が不変）。
        let after = read_registry(&registry_path)?;
        assert_eq!(
            after.lookup("neovim", DeltaSource::NixEval),
            Some(&saved_entry)
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn record_computes_brew_cask_delta_and_records_version_only() -> Result<()> {
        // homebrew.nix + 両 cask rev から cask 版差分を Rust（reqwest seam）で算出し、ノート未取得は
        // version-only として記録する経路を network 抜きで固定する。
        let dir = temp_dir("brew");
        let homebrew = write_nix(&dir, "homebrew.nix", "casks = [ \"azookey\" ];");
        let out = dir.join("2026-06.toml");
        let registry = dir.join("notes-sources.toml");
        let mut inp = input(&dir, &out, &registry, None, None);
        inp.homebrew_nix = Some(&homebrew);
        inp.cask_rev_old = Some("oldrev");
        inp.cask_rev_new = Some("newrev");
        // cask fetch seam: rev に応じて azookey の version 文字列を返す（upgrade）。
        let fetch_cask = |url: &str| -> Result<Option<String>> {
            Ok(if url.contains("/oldrev/") {
                Some("cask \"x\" do\n  version \"1.0\"\nend\n".to_string())
            } else if url.contains("/newrev/") {
                Some("cask \"x\" do\n  version \"1.1\"\nend\n".to_string())
            } else {
                None
            })
        };
        let extract = FakeExtractor::new();
        run_record_with(
            &inp,
            &extract,
            &no_fetch_source,
            &no_brew_hint,
            &no_eval,
            &fetch_cask,
        )?;
        let entries = read_entries(&out)?;
        let pkg = &entries[0].packages[0];
        assert_eq!(pkg.name, "azookey");
        assert_eq!(pkg.source, PackageSource::Brew);
        assert_eq!(pkg.old.as_deref(), Some("1.0"));
        assert_eq!(pkg.new.as_deref(), Some("1.1"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn record_evals_nix_new_via_seam_when_file_absent() -> Result<()> {
        // `--nix-new` 未指定なら eval seam（本番は nix eval）で bump 後版を得る経路を固定する。
        let dir = temp_dir("eval-new");
        let old = write_nix(&dir, "old.json", r#"{"neovim":{"version":"0.10"}}"#);
        let out = dir.join("2026-06.toml");
        let registry = dir.join("notes-sources.toml");
        let mut inp = input(&dir, &out, &registry, Some(&old), None);
        inp.nix_new = None;
        let eval_new =
            |_reference: &str| -> Result<std::collections::BTreeMap<String, NixPackage>> {
                let mut map = std::collections::BTreeMap::new();
                map.insert(
                    "neovim".to_string(),
                    NixPackage {
                        version: "0.11".to_string(),
                        repo: String::new(),
                        notes_source: String::new(),
                        homepage: String::new(),
                    },
                );
                Ok(map)
            };
        let extract = FakeExtractor::new();
        run_record_with(
            &inp,
            &extract,
            &no_fetch_source,
            &no_brew_hint,
            &eval_new,
            &no_cask,
        )?;
        let entries = read_entries(&out)?;
        let pkg = &entries[0].packages[0];
        assert_eq!(pkg.name, "neovim");
        assert_eq!(pkg.new.as_deref(), Some("0.11"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    // ---- registry / store の単体固定 ----

    fn entry_of(source: Option<&str>, origin: NotesOrigin) -> NotesSourceEntry {
        NotesSourceEntry {
            source: source.map(str::to_string),
            origin,
            discovered_at: None,
            note: None,
        }
    }

    #[test]
    fn reusable_source_only_for_resolved_origins_and_keeps_sources_separate_by_origin() {
        assert_eq!(
            entry_of(
                Some("https://github.com/o/r/releases"),
                NotesOrigin::Mechanical
            )
            .reusable_source(),
            Some("https://github.com/o/r/releases")
        );
        assert_eq!(
            entry_of(
                Some("https://github.com/o/r/blob/x"),
                NotesOrigin::AiDiscovered
            )
            .reusable_source(),
            Some("https://github.com/o/r/blob/x")
        );
        assert_eq!(entry_of(None, NotesOrigin::None).reusable_source(), None);
        assert_eq!(
            entry_of(Some("https://github.com/o/r"), NotesOrigin::None).reusable_source(),
            None
        );
    }

    #[test]
    fn registry_upserts_keeps_nix_brew_separate_and_serializes_in_name_order() -> Result<()> {
        let mut registry = NotesSourceRegistry::default();
        assert!(registry.lookup("neovim", DeltaSource::NixEval).is_none());
        // 同名 nix/brew は別キー。
        registry.record(
            "firefox",
            DeltaSource::NixEval,
            entry_of(
                Some("https://github.com/mozilla/firefox/releases"),
                NotesOrigin::Mechanical,
            ),
        );
        registry.record(
            "firefox",
            DeltaSource::BrewTap,
            entry_of(
                Some("https://github.com/homebrew/homebrew-cask/blob/x/firefox.rb"),
                NotesOrigin::AiDiscovered,
            ),
        );
        assert_eq!(registry_key("firefox", DeltaSource::NixEval), "nix/firefox");
        assert_eq!(
            registry_key("firefox", DeltaSource::BrewTap),
            "brew/firefox"
        );
        assert_eq!(
            registry
                .lookup("firefox", DeltaSource::BrewTap)
                .and_then(NotesSourceEntry::reusable_source),
            Some("https://github.com/homebrew/homebrew-cask/blob/x/firefox.rb")
        );

        // 名前昇順で決定論直列化（バイト固定）。
        let mut ordered = NotesSourceRegistry::default();
        ordered.record(
            "ripgrep",
            DeltaSource::NixEval,
            entry_of(
                Some("https://github.com/BurntSushi/ripgrep/releases"),
                NotesOrigin::Mechanical,
            ),
        );
        ordered.record(
            "bat",
            DeltaSource::NixEval,
            entry_of(
                Some("https://github.com/sharkdp/bat/releases"),
                NotesOrigin::AiDiscovered,
            ),
        );
        ordered.record(
            "zlib",
            DeltaSource::NixEval,
            entry_of(None, NotesOrigin::None),
        );
        let rendered = toml::to_string(&ordered)?;
        let expected = "\
[\"nix/bat\"]
source = \"https://github.com/sharkdp/bat/releases\"
origin = \"ai-discovered\"

[\"nix/ripgrep\"]
source = \"https://github.com/BurntSushi/ripgrep/releases\"
origin = \"mechanical\"

[\"nix/zlib\"]
origin = \"none\"
";
        assert_eq!(rendered, expected);
        let parsed: NotesSourceRegistry = toml::from_str(&rendered)?;
        assert_eq!(parsed, ordered);
        Ok(())
    }

    fn store_sample(at: &str, name: &str) -> UpdateEntry {
        UpdateEntry {
            at: at.to_string(),
            nixpkgs_old: "o".to_string(),
            nixpkgs_new: "n".to_string(),
            reference: "darwinConfigurations.ci".to_string(),
            severity: super::super::wire::Severity::Minor,
            overall: "1アプリ更新: ✨1".to_string(),
            packages: vec![PackageUpdate {
                name: name.to_string(),
                old: Some("1.0".to_string()),
                new: Some("1.1".to_string()),
                change: super::super::wire::ChangeKind::Upgraded,
                declared: true,
                source: PackageSource::Nix,
                notes_url: None,
                change_items: Vec::new(),
            }],
        }
    }

    #[test]
    fn store_read_append_and_monthly_directory_filter() -> Result<()> {
        let dir = temp_dir("store");
        // 不存在は空。
        assert!(read_entries(&dir.join("missing.toml"))?.is_empty());
        // append は既存保持で累積。
        let path = dir.join("2026-06.toml");
        append_entry(&path, &store_sample("2026-06-01T00:00:00Z", "a"))?;
        append_entry(&path, &store_sample("2026-06-02T00:00:00Z", "b"))?;
        let entries = read_entries(&path)?;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].packages[0].name, "b");
        // directory 読みは registry/非月次を除外する。
        std::fs::write(
            dir.join("notes-sources.toml"),
            "[\"nix/x\"]\norigin = \"none\"\n",
        )
        .ok();
        std::fs::write(dir.join("scratch.toml"), "key = \"value\"\n").ok();
        assert!(is_monthly_history_file("2026-06.toml"));
        assert!(!is_monthly_history_file("notes-sources.toml"));
        assert!(!is_monthly_history_file("2026-6.toml"));
        let dir_entries = read_entries(&dir)?;
        assert_eq!(dir_entries.len(), 2);
        assert_eq!(dir_entries[0].packages[0].name, "a");
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn registry_round_trips_through_file() -> Result<()> {
        let dir = temp_dir("registry-file");
        let path = dir.join("notes-sources.toml");
        assert_eq!(read_registry(&path)?, NotesSourceRegistry::default());
        let mut registry = NotesSourceRegistry::default();
        registry.record(
            "neovim",
            DeltaSource::NixEval,
            entry_of(
                Some("https://github.com/neovim/neovim/releases"),
                NotesOrigin::Mechanical,
            ),
        );
        write_registry(&path, &registry)?;
        assert_eq!(read_registry(&path)?, registry);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
