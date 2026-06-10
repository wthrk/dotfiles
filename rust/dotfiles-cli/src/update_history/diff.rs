//! nix eval 由来の name→version マップ比較と、brew tap rev 版差分の表現型・純粋規則。
//!
//! 外部 I/O を持たない純粋な比較とマージ、および version 比較/範囲判定の domain rule を置く。nix eval
//! プロセス実行・eval JSON 取得・brew 版差分ファイル読み取りは [`super::notes`] の取得関数が担い、本 module は
//! 取得済み値を version 差分モデルへ翻訳する規則だけを固定する。version 差分の意味論（added/removed/
//! upgraded/downgraded）はここが正本である。

use std::collections::BTreeMap;

use super::wire::ChangeKind;

/// 差分 version の出所（nix eval か Homebrew tap rev か）。
///
/// nix=eval と brew=tap rev 版差分は同じ version 差分モデルへ統合されるが、出所により
/// ノート取得先（forge releases / cask homepage）が変わるため、出所だけは型として保持する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeltaSource {
    /// `nix eval` で取得した宣言パッケージの name→version 差分由来。
    NixEval,
    /// Homebrew tap rev の formula/cask ファイル差分由来。
    BrewTap,
}

impl DeltaSource {
    /// 出所の安定キー（provenance レジストリのキー名前空間に使う）。
    ///
    /// nix 由来と brew 由来は同名でも別パッケージ（例: nix の `firefox` と cask の `firefox`）であり、ノート
    /// 取得元 provenance を name だけで突合すると別出所の取得元を取り違える。`Debug` 表現でなくこの値を使い、
    /// variant 名がリファクタで変わってもキーは不変にする（決定論の根拠）。
    pub(crate) fn as_stable_key(self) -> &'static str {
        match self {
            DeltaSource::NixEval => "nix",
            DeltaSource::BrewTap => "brew",
        }
    }
}

/// `nix eval` が宣言パッケージごとに返す評価時属性（version・GitHub owner/repo・changelog URL・homepage）。
///
/// `version` は `pname`/`version`（取れなければ空文字）。`repo` は GitHub `owner/repo`（無ければ空文字）で
/// Releases API の一次取得元。`notes_source` は changelog URL（Releases API 空振り時の raw フォールバック取得先）。
/// `homepage` は AI エージェントの fetch 許可ホスト集合のヒント。いずれも信頼境界外の値であり、実取得時に host
/// allowlist で機械検証する。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct NixPackage {
    /// 評価時 version（無ければ空文字。空は版不明 = `None` 扱い）。
    pub(crate) version: String,
    /// GitHub `owner/repo`（無ければ空文字）。
    #[serde(default)]
    pub(crate) repo: String,
    /// changelog URL（無ければ空文字）。eval JSON では `changelog` key で serialize / deserialize し、`notes_source`
    /// key も alias で受ける。Rust フィールド名は `notes_source`。
    #[serde(default, rename = "changelog", alias = "notes_source")]
    pub(crate) notes_source: String,
    /// `meta.homepage` 由来の URL（無ければ空文字）。AI エージェントの fetch 許可ホスト集合のヒント。
    #[serde(default)]
    pub(crate) homepage: String,
}

/// 単一パッケージの version 差分（比較/マージの中間表現）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VersionDelta {
    /// パッケージ名。
    pub(crate) name: String,
    /// 更新前 version（不在なら `None`）。
    pub(crate) old: Option<String>,
    /// 更新後 version（不在なら `None`）。
    pub(crate) new: Option<String>,
    /// version 差分の種別。
    pub(crate) change: ChangeKind,
    /// 差分の出所（nix eval / brew tap）。
    pub(crate) source: DeltaSource,
    /// nix eval 由来の GitHub `owner/repo`（無ければ `None`）。brew は `None`。
    pub(crate) repo: Option<String>,
    /// nix eval 由来の changelog URL（無ければ `None`）。brew は `None`。
    pub(crate) notes_source: Option<String>,
    /// nix eval 由来の homepage URL（無ければ `None`）。brew は `None`。
    pub(crate) homepage: Option<String>,
}

/// nix eval 由来 / brew tap 由来の version 差分を同一モデルへ統合する（nix を先に、brew を後に）。
pub(crate) fn merge_version_deltas(
    nix: Vec<VersionDelta>,
    brew: Vec<VersionDelta>,
) -> Vec<VersionDelta> {
    nix.into_iter().chain(brew).collect()
}

/// old/new の宣言パッケージ name→version マップを比較して [`VersionDelta`] 列へ変換する純粋関数。
///
/// new のみ→Added、old のみ→Removed、両方在り version が異なる→大小比較で Upgraded/Downgraded、等しい→除外。
/// 出力は名前昇順（`BTreeMap` 反復順）で決定論的。`repo`/`notes_source`/`homepage` は new 側の値を運ぶ
/// （removed は new が無いため `None`）。
pub(crate) fn diff_versions(
    old: &BTreeMap<String, NixPackage>,
    new: &BTreeMap<String, NixPackage>,
) -> Vec<VersionDelta> {
    // new 側（Added / Upgraded / Downgraded。版不変は除外）を name 昇順で先に、続けて old のみ（Removed）を name
    // 昇順で連結する。いずれも `BTreeMap` 反復順なので決定論的。
    let changed = new
        .iter()
        .filter_map(|(name, new_pkg)| changed_delta(name, old.get(name), new_pkg));
    let removed = old
        .iter()
        .filter(|(name, _)| !new.contains_key(*name))
        .map(|(name, old_pkg)| removed_delta(name, old_pkg));
    changed.chain(removed).collect()
}

/// new 側 1 件を、old の有無と版比較から `Added`/`Upgraded`/`Downgraded` の delta へ翻訳する（版不変は `None`）。
fn changed_delta(
    name: &str,
    old_pkg: Option<&NixPackage>,
    new_pkg: &NixPackage,
) -> Option<VersionDelta> {
    let repo = empty_to_none(&new_pkg.repo);
    let notes_source = empty_to_none(&new_pkg.notes_source);
    let homepage = empty_to_none(&new_pkg.homepage);
    match old_pkg {
        Some(old_pkg) if old_pkg.version == new_pkg.version => None,
        Some(old_pkg) => Some(VersionDelta {
            name: name.to_string(),
            old: version_value(&old_pkg.version),
            new: version_value(&new_pkg.version),
            change: compare_versions(&old_pkg.version, &new_pkg.version),
            source: DeltaSource::NixEval,
            repo,
            notes_source,
            homepage,
        }),
        None => Some(VersionDelta {
            name: name.to_string(),
            old: None,
            new: version_value(&new_pkg.version),
            change: ChangeKind::Added,
            source: DeltaSource::NixEval,
            repo,
            notes_source,
            homepage,
        }),
    }
}

/// old のみに存在するパッケージを `Removed` delta へ翻訳する（new が無いので取得元は運ばない）。
fn removed_delta(name: &str, old_pkg: &NixPackage) -> VersionDelta {
    VersionDelta {
        name: name.to_string(),
        old: version_value(&old_pkg.version),
        new: None,
        change: ChangeKind::Removed,
        source: DeltaSource::NixEval,
        repo: None,
        notes_source: None,
        homepage: None,
    }
}

/// version 属性が空文字なら `None`、それ以外は版文字列を返す。
fn version_value(version: &str) -> Option<String> {
    if version.is_empty() {
        None
    } else {
        Some(version.to_string())
    }
}

/// 空文字を `None`、それ以外を値として返す（偽の取得元を運ばない）。
fn empty_to_none(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// 両側に存在する 2 version 文字列を比較し、昇格か降格かを決める。
///
/// 文字列が完全一致する差分は呼び出し側（[`diff_versions`]）で除外済みのため、ここに来る old/new は必ず異なる。
/// `version_ordering` が `Equal`（区切り記号差など成分が同値）になったときは方向を `Upgraded` に固定せず、
/// 生文字列の安定タイブレークで向きを決める（`old < new` を Upgraded、それ以外を Downgraded）。
fn compare_versions(old: &str, new: &str) -> ChangeKind {
    match version_ordering(old, new) {
        std::cmp::Ordering::Greater => ChangeKind::Downgraded,
        std::cmp::Ordering::Less => ChangeKind::Upgraded,
        std::cmp::Ordering::Equal => match old.cmp(new) {
            std::cmp::Ordering::Greater => ChangeKind::Downgraded,
            // 文字列も等しいケースは呼び出し側で除外済み。残りは Upgraded に倒す（安定タイブレーク）。
            std::cmp::Ordering::Less | std::cmp::Ordering::Equal => ChangeKind::Upgraded,
        },
    }
}

/// 2 version 文字列の順序を比較する（semver の prerelease 規則に沿う）。
///
/// release 部（最初の `-` より前）を成分単位で比較し、全成分が等しく長さだけ異なるときは成分数が多い側を
/// 新しいとみなす（`1.2` < `1.2.1`）。release 部が等しいときは prerelease 規則を適用する: prerelease 付き
/// （`1.0.0-rc1`）は同じ release の stable（`1.0.0`）より **低い**。双方 prerelease のときは prerelease 成分の
/// 辞書/数値比較で順序づける。版比較・範囲判定の単一の正本であり、brew 差分・release 範囲フィルタも共有する。
pub(crate) fn version_ordering(lhs: &str, rhs: &str) -> std::cmp::Ordering {
    let (lhs_release, lhs_pre) = split_release_pre(lhs);
    let (rhs_release, rhs_pre) = split_release_pre(rhs);
    let release_order = compare_parts(&lhs_release, &rhs_release);
    if release_order != std::cmp::Ordering::Equal {
        return release_order;
    }
    // release 部が同値: prerelease 無しは prerelease 有りより新しい（stable > prerelease）。
    match (lhs_pre.is_empty(), rhs_pre.is_empty()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => compare_parts(&lhs_pre, &rhs_pre),
    }
}

/// version 文字列を release 部（最初の `-` より前）と prerelease 部（以降）の成分列へ分ける。
///
/// release 部は `.` 区切り、prerelease 部は `.` と `-` 区切りで成分化する（空成分は除く）。
fn split_release_pre(version: &str) -> (Vec<&str>, Vec<&str>) {
    let (release, pre) = match version.split_once('-') {
        Some((release, pre)) => (release, pre),
        None => (version, ""),
    };
    let release_parts: Vec<&str> = release.split('.').filter(|s| !s.is_empty()).collect();
    let pre_parts: Vec<&str> = pre.split(['.', '-']).filter(|s| !s.is_empty()).collect();
    (release_parts, pre_parts)
}

/// 成分列を先頭から比較し、全成分が等しいときは成分数が多い側を新しいとみなす。
fn compare_parts(lhs_parts: &[&str], rhs_parts: &[&str]) -> std::cmp::Ordering {
    for (l, r) in lhs_parts.iter().zip(rhs_parts.iter()) {
        let ordering = compare_component(l, r);
        if ordering != std::cmp::Ordering::Equal {
            return ordering;
        }
    }
    lhs_parts.len().cmp(&rhs_parts.len())
}

/// 1 成分を比較する（両方数値なら数値比較、片方のみ数値なら数値側を小さく、いずれも非数値なら辞書順）。
fn compare_component(lhs: &str, rhs: &str) -> std::cmp::Ordering {
    match (lhs.parse::<u64>(), rhs.parse::<u64>()) {
        (Ok(l), Ok(r)) => l.cmp(&r),
        (Ok(_), Err(_)) => std::cmp::Ordering::Less,
        (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
        (Err(_), Err(_)) => lhs.cmp(rhs),
    }
}

/// tag/name 文字列から version 様トークンを抽出して正規化する純粋関数。
///
/// label と version を分ける区切り（空白・`_`）で分割し、末尾トークンから version 開始位置を探す。`-` は
/// 区切りとして消費せず、prerelease suffix（`v1.0.0-rc1` の `-rc1`）を version の一部として保持する。
/// label 接頭辞（`pkg-v1.5.0` の `pkg-`・`release-1.0` の `release-`）は version 開始 segment まで読み飛ばす。
fn extract_version_token(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.split([' ', '_']).rev().find_map(version_from_token)
}

/// 1 トークン（`pkg-v1.5.0` / `v1.0.0-rc1` 等）から version 開始 segment 以降を取り出し正規化する純粋関数。
///
/// `-` 区切りの segment を左から走査し、`v`/`V` を剥がして数字始まりになる最初の segment を version 開始と
/// みなす。その segment 以降（prerelease suffix を含む）を `-` で再結合し正規化して返す（version 様が無ければ
/// `None`）。これにより label 接頭辞は読み飛ばしつつ prerelease suffix は保持する。
fn version_from_token(token: &str) -> Option<String> {
    let segments: Vec<&str> = token.split('-').collect();
    let start = segments
        .iter()
        .position(|segment| starts_with_version_digit(segment))?;
    Some(normalize_version(&segments[start..].join("-")))
}

/// segment が（先頭の `v`/`V` を剥がしたうえで）数字始まりかを判定する純粋関数。
fn starts_with_version_digit(segment: &str) -> bool {
    normalize_version(segment)
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit())
}

/// version 文字列を正規化する純粋関数（先頭の `v`/`V` を剥がし、前後空白を除く）。
fn normalize_version(version: &str) -> String {
    let trimmed = version.trim();
    trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed)
        .to_string()
}

/// release tag（無ければ name）から version 文字列を抽出して正規化する純粋関数。
pub(crate) fn release_version(tag: &str, name: &str) -> Option<String> {
    extract_version_token(tag).or_else(|| extract_version_token(name))
}

/// 抽出済み release version が `(old, new]`（old 排他・new 包含）に入るかを判定する純粋関数。
pub(crate) fn version_in_range(version: &str, old: Option<&str>, new: Option<&str>) -> bool {
    if let Some(old) = old.map(normalize_version)
        && version_ordering(version, &old) != std::cmp::Ordering::Greater
    {
        return false;
    }
    if let Some(new) = new.map(normalize_version)
        && version_ordering(version, &new) == std::cmp::Ordering::Greater
    {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    //! eval マップ比較の種別確定・version 欠落フォールバック・マージ順序・版比較/範囲判定を固定する。

    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, NixPackage> {
        pairs
            .iter()
            .map(|(name, version)| {
                (
                    (*name).to_string(),
                    NixPackage {
                        version: (*version).to_string(),
                        repo: String::new(),
                        notes_source: String::new(),
                        homepage: String::new(),
                    },
                )
            })
            .collect()
    }

    fn map_with_notes(pairs: &[(&str, &str, &str, &str)]) -> BTreeMap<String, NixPackage> {
        pairs
            .iter()
            .map(|(name, version, repo, notes)| {
                (
                    (*name).to_string(),
                    NixPackage {
                        version: (*version).to_string(),
                        repo: (*repo).to_string(),
                        notes_source: (*notes).to_string(),
                        homepage: String::new(),
                    },
                )
            })
            .collect()
    }

    fn find<'a>(deltas: &'a [VersionDelta], name: &str) -> Option<&'a VersionDelta> {
        deltas.iter().find(|d| d.name == name)
    }

    #[test]
    fn diff_detects_added_removed_upgraded_downgraded() {
        let old = map(&[("neovim", "0.10.2"), ("oldpkg", "1.0.0"), ("ruby", "3.4.0")]);
        let new = map(&[
            ("neovim", "0.11.0"),
            ("ripgrep", "14.1.0"),
            ("ruby", "3.3.10"),
        ]);
        let deltas = diff_versions(&old, &new);
        assert_eq!(deltas.len(), 4);
        assert_eq!(
            find(&deltas, "neovim").map(|d| d.change),
            Some(ChangeKind::Upgraded)
        );
        assert_eq!(
            find(&deltas, "ripgrep").map(|d| d.change),
            Some(ChangeKind::Added)
        );
        assert_eq!(find(&deltas, "ripgrep").map(|d| d.old.clone()), Some(None));
        assert_eq!(
            find(&deltas, "ruby").map(|d| d.change),
            Some(ChangeKind::Downgraded)
        );
        assert_eq!(
            find(&deltas, "oldpkg").map(|d| d.change),
            Some(ChangeKind::Removed)
        );
        assert_eq!(find(&deltas, "oldpkg").map(|d| d.new.clone()), Some(None));
    }

    #[test]
    fn diff_excludes_unchanged_versions() {
        let old = map(&[("zlib", "1.3.1"), ("git", "2.53.0")]);
        let new = map(&[("zlib", "1.3.1"), ("git", "2.54.0")]);
        let deltas = diff_versions(&old, &new);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].name, "git");
    }

    #[test]
    fn diff_treats_empty_version_as_absent() {
        let old = map(&[("google-cloud-sdk", ""), ("python3", "")]);
        let new = map(&[("google-cloud-sdk", "500.0.0"), ("python3", "")]);
        let deltas = diff_versions(&old, &new);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].name, "google-cloud-sdk");
        assert_eq!(deltas[0].old, None);
        assert_eq!(deltas[0].new.as_deref(), Some("500.0.0"));
    }

    #[test]
    fn diff_output_is_name_sorted_for_determinism() {
        let old = map(&[("a", "1"), ("c", "1")]);
        let new = map(&[("a", "2"), ("b", "1"), ("c", "2")]);
        let deltas = diff_versions(&old, &new);
        let names: Vec<&str> = deltas.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn diff_handles_multi_component_versions() {
        let old = map(&[("pkg", "1.2"), ("nodejs", "22.22.2")]);
        let new = map(&[("pkg", "1.2.1"), ("nodejs", "22.22.10")]);
        let deltas = diff_versions(&old, &new);
        assert!(deltas.iter().all(|d| d.change == ChangeKind::Upgraded));
    }

    #[test]
    fn diff_carries_new_side_repo_and_notes_source_into_delta() {
        let old = map_with_notes(&[(
            "neovim",
            "0.10",
            "neovim/neovim",
            "https://github.com/neovim/neovim/blob/master/CHANGELOG",
        )]);
        let new = map_with_notes(&[
            (
                "neovim",
                "0.11",
                "neovim/neovim",
                "https://github.com/neovim/neovim/blob/master/CHANGELOG",
            ),
            ("nonotes", "2.0", "", ""),
        ]);
        let deltas = diff_versions(&old, &new);
        assert_eq!(
            find(&deltas, "neovim").and_then(|d| d.repo.as_deref()),
            Some("neovim/neovim")
        );
        assert_eq!(find(&deltas, "nonotes").map(|d| d.repo.clone()), Some(None));
    }

    #[test]
    fn prerelease_is_lower_than_same_stable_release() {
        // semver: prerelease（`1.0.0-rc1`）は同 release の stable（`1.0.0`）より低い。
        assert_eq!(
            version_ordering("1.0.0-rc1", "1.0.0"),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            version_ordering("1.0.0", "1.0.0-rc1"),
            std::cmp::Ordering::Greater
        );
        // 双方 prerelease は prerelease 成分で比較する（rc1 < rc2）。
        assert_eq!(
            version_ordering("1.0.0-rc1", "1.0.0-rc2"),
            std::cmp::Ordering::Less
        );
        // release 成分の差は prerelease の有無より優先する。
        assert_eq!(
            version_ordering("1.0.0", "1.1.0-rc1"),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn prerelease_to_stable_is_upgrade_not_downgrade() {
        // `1.0.0-rc1` → `1.0.0` は upgrade（成分数だけ見て downgrade 誤分類しない）。
        let old = map(&[("pkg", "1.0.0-rc1")]);
        let new = map(&[("pkg", "1.0.0")]);
        let deltas = diff_versions(&old, &new);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].change, ChangeKind::Upgraded);
    }

    #[test]
    fn equal_ordering_does_not_force_upgraded_direction() {
        // 区切り記号差（末尾区切りなど）で成分が同値=Equal になる異なる文字列。向きは生文字列の安定
        // タイブレークで決め、Upgraded 固定にしない。
        assert_eq!(version_ordering("1.0.", "1.0"), std::cmp::Ordering::Equal);
        // old > new（文字列降順）の Equal は Downgraded、old < new は Upgraded に倒れる。
        assert_eq!(compare_versions("1.0.", "1.0"), ChangeKind::Downgraded);
        assert_eq!(compare_versions("1.0", "1.0."), ChangeKind::Upgraded);
    }

    #[test]
    fn normalize_version_strips_v_prefix_and_whitespace() {
        assert_eq!(normalize_version("v1.2.3"), "1.2.3");
        assert_eq!(normalize_version("V2.0"), "2.0");
        assert_eq!(normalize_version("  1.0  "), "1.0");
        assert_eq!(normalize_version("3.4.0"), "3.4.0");
    }

    #[test]
    fn extract_version_token_picks_last_version_like_token() {
        assert_eq!(extract_version_token("v1.2.3").as_deref(), Some("1.2.3"));
        assert_eq!(
            extract_version_token("pkg-v1.5.0").as_deref(),
            Some("1.5.0")
        );
        assert_eq!(extract_version_token("release-1.0").as_deref(), Some("1.0"));
        assert_eq!(extract_version_token("latest"), None);
        // prerelease suffix（`-rc1` 等）は version の一部として保持する（label 接頭辞のみ読み飛ばす）。
        assert_eq!(
            extract_version_token("v1.0.0-rc1").as_deref(),
            Some("1.0.0-rc1")
        );
        assert_eq!(
            extract_version_token("pkg-v2.0.0-beta.2").as_deref(),
            Some("2.0.0-beta.2")
        );
        assert_eq!(
            extract_version_token("Release 3.1.0-alpha").as_deref(),
            Some("3.1.0-alpha")
        );
    }

    #[test]
    fn release_version_preserves_prerelease_suffix() {
        // GitHub release tag/name から prerelease suffix を含む semver 全体を抽出する（suffix を捨てない）。
        assert_eq!(
            release_version("v1.0.0-rc1", "anything").as_deref(),
            Some("1.0.0-rc1")
        );
        assert_eq!(
            release_version("latest", "v2.0.0-beta.1").as_deref(),
            Some("2.0.0-beta.1")
        );
    }

    #[test]
    fn prerelease_not_misincluded_into_stable_range() {
        // `(1.0.0-rc1, 1.0.0]` 範囲: stable `1.0.0` は包含、prerelease `1.0.0-rc1`（=old 境界）は old 排他で除外。
        let old = Some("1.0.0-rc1");
        let new = Some("1.0.0");
        // old 境界の prerelease 自身は old 排他なので入らない。
        assert!(!version_in_range("1.0.0-rc1", old, new));
        // stable は範囲に入る（prerelease より新しい）。
        assert!(version_in_range("1.0.0", old, new));
        // 別 release の prerelease（`1.0.0-rc2`）は old(`1.0.0-rc1`)より新しく new(`1.0.0`)以下なので入る。
        assert!(version_in_range("1.0.0-rc2", old, new));
        // 逆向き: `(1.0.0, 1.1.0]` に `1.1.0-rc1` は stable `1.1.0` より低いため new 包含側に入る（誤って外さない）。
        assert!(version_in_range("1.1.0-rc1", Some("1.0.0"), Some("1.1.0")));
        // 次 stable の prerelease（`1.1.0-rc1`）は old=`1.1.0` のとき old 以下なので除外（rc は stable に混入しない）。
        assert!(!version_in_range("1.1.0-rc1", Some("1.1.0"), Some("1.2.0")));
    }

    #[test]
    fn release_version_prefers_tag_then_name() {
        assert_eq!(
            release_version("v1.2.3", "anything").as_deref(),
            Some("1.2.3")
        );
        assert_eq!(
            release_version("latest", "Release 2.0.0").as_deref(),
            Some("2.0.0")
        );
        assert_eq!(release_version("latest", "nightly"), None);
    }

    #[test]
    fn version_in_range_is_old_exclusive_new_inclusive() {
        let old = Some("1.0.0");
        let new = Some("1.2.0");
        assert!(!version_in_range("1.0.0", old, new));
        assert!(version_in_range("1.1.0", old, new));
        assert!(version_in_range("1.2.0", old, new));
        assert!(!version_in_range("0.9.0", old, new));
        assert!(!version_in_range("1.3.0", old, new));
    }

    #[test]
    fn version_in_range_normalizes_boundary_tag_variants() {
        assert!(version_in_range("1.5.0", Some("v1.0.0"), Some("v2.0.0")));
        assert!(!version_in_range("2.5.0", Some("v1.0.0"), Some("v2.0.0")));
    }

    #[test]
    fn version_in_range_with_unbounded_old_or_new() {
        assert!(version_in_range("0.1.0", None, Some("1.0.0")));
        assert!(version_in_range("9.9.9", Some("1.0.0"), None));
        assert!(version_in_range("5.0.0", None, None));
    }

    #[test]
    fn merge_keeps_nix_first_then_brew() {
        let nix = vec![VersionDelta {
            name: "neovim".to_string(),
            old: Some("0.10".to_string()),
            new: Some("0.11".to_string()),
            change: ChangeKind::Upgraded,
            source: DeltaSource::NixEval,
            repo: None,
            notes_source: None,
            homepage: None,
        }];
        let brew = vec![VersionDelta {
            name: "firefox".to_string(),
            old: Some("120".to_string()),
            new: Some("121".to_string()),
            change: ChangeKind::Upgraded,
            source: DeltaSource::BrewTap,
            repo: None,
            notes_source: None,
            homepage: None,
        }];
        let merged = merge_version_deltas(nix, brew);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].source, DeltaSource::NixEval);
        assert_eq!(merged[1].source, DeltaSource::BrewTap);
    }
}
