//! nix eval 由来の name→version マップ比較と、brew tap rev 版差分の表現型。
//!
//! ここに置くのは外部 I/O を持たない純粋な比較とマージだけである。nix eval プロセス実行・eval
//! JSON 取得（`adapters/nix.rs`）、tap rev からの formula/cask 版差分読み取り（`adapters/brew.rs`）は
//! adapter の責務であり、本 module は取得済み値を version 差分モデルへ翻訳する規則だけを domain rule
//! として固定する。version 差分の意味論（added/removed/upgraded/downgraded）はここが正本である。

use std::collections::BTreeMap;

use super::wire::ChangeKind;

/// 差分 version の出所（nix eval か Homebrew tap rev か）。
///
/// nix=eval と brew=tap rev 版差分は同じ version 差分モデルへ統合されるが、出所により
/// ノート取得先（forge releases / cask homepage）が変わるため、出所だけは型として保持する。
/// 実取得は adapter（`adapters/nix.rs`・`adapters/brew.rs`）が行い、本 module は出所タグ付けまでを担う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeltaSource {
    /// `nix eval` で取得した宣言パッケージの name→version 差分由来。
    NixEval,
    /// Homebrew tap rev の formula/cask ファイル差分由来。
    BrewTap,
}

/// `nix eval` が宣言パッケージごとに返す評価時属性（version・GitHub owner/repo・changelog URL）。
///
/// `version` は `pname`/`version`（無ければ `parseDrvName` フォールバック、版が取れなければ空文字）。
/// `repo` は当該パッケージの GitHub `owner/repo`（`meta.homepage`→`src`→`meta.changelog` の優先で eval が
/// 抽出。github 由来が取れなければ空文字）であり、GitHub Releases API で old→new 範囲のリリースノートを引く
/// 一次取得元になる。`notes_source`（旧 JSON key、現 `changelog`）は `meta.changelog`（無ければ
/// `meta.homepage`）の URL で、Releases API が空振りしたときの changelog raw フォールバック取得先になる。
/// version 比較は `version` だけで行い、`repo`/`notes_source` は new 側の値を [`VersionDelta`] のノート取得元
/// として運ぶ（いずれも信頼境界外の値であり、実取得時に host allowlist で機械検証する）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub(crate) struct NixPackage {
    /// 評価時 version（無ければ空文字。空は版不明 = `None` 扱い）。
    pub(crate) version: String,
    /// GitHub `owner/repo`（eval が `meta.homepage`/`src`/`meta.changelog` から抽出。無ければ空文字）。
    #[serde(default)]
    pub(crate) repo: String,
    /// `meta.changelog` または `meta.homepage` 由来の changelog URL（無ければ空文字）。eval JSON では
    /// `changelog` key で出力する（旧 `notes_source` key も alias で受ける）。Releases API 空振り時の
    /// changelog raw フォールバック取得先であり、AI エージェントの fetch 許可ホスト集合ヒントにもなる。
    #[serde(default, alias = "changelog")]
    pub(crate) notes_source: String,
    /// `meta.homepage` 由来の URL（無ければ空文字）。AI エージェント（GitHub Models tool-use ループ）の
    /// fetch 許可ホスト集合のヒントになる（このパッケージの正規ドメインを許可するため）。信頼境界内（eval 由来）。
    #[serde(default)]
    pub(crate) homepage: String,
}

/// 単一パッケージの version 差分（比較/マージの中間表現）。
///
/// `old` / `new` は version が不在のとき `None`。`change` は両側の存在有無と version 文字列の
/// 大小比較から確定する種別である。`source` は nix/brew いずれの差分系統かを示し、両系統を同一
/// モデルへマージしてもノート取得先を区別できるようにする。`notes_source` は nix eval 由来 delta が
/// 運ぶ当該パッケージのノート取得先 URL（`meta.changelog`/`meta.homepage` 由来。無ければ `None`）であり、
/// brew tap 由来 delta では `None`（brew は cask base + name でノート URL を解決するため delta には持たせない）。
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
    /// nix eval 由来の GitHub `owner/repo`（Releases API の一次取得元。無ければ `None`）。brew は `None`。
    pub(crate) repo: Option<String>,
    /// nix eval 由来の changelog URL（`meta.changelog`/`meta.homepage`。Releases API 空振り時の
    /// フォールバック取得先。無ければ `None`）。brew は `None`。
    pub(crate) notes_source: Option<String>,
    /// nix eval 由来の homepage URL（`meta.homepage`。AI エージェントの fetch 許可ホスト集合ヒント。
    /// 無ければ `None`）。brew は `None`。信頼境界内（eval 由来）。
    pub(crate) homepage: Option<String>,
}

/// nix eval 由来 / brew tap 由来の version 差分を同一モデルへ統合する。
///
/// 同名パッケージが両系統に現れた場合でも、出所が異なれば別エントリとして保持する（nix の `firefox`
/// と cask の `firefox` のように意味が異なりうるため domain では併合しない）。並びは nix 差分を先に、
/// 次に brew 差分を、各系統内では入力順を保つ。実差分取得は adapter の責務であり、本関数は取得済み
/// 2 系統の結合順序だけを domain rule として固定する。
pub(crate) fn merge_version_deltas(
    nix: Vec<VersionDelta>,
    brew: Vec<VersionDelta>,
) -> Vec<VersionDelta> {
    let mut merged = nix;
    merged.extend(brew);
    merged
}

/// old/new の宣言パッケージ name→version マップを比較して [`VersionDelta`] 列へ変換する純粋関数。
///
/// nightly が欲しいのは「どの宣言パッケージが old→new で版変化したか」だけであり、それは `nix eval`
/// で評価時属性（`pname`/`version`）として数秒で取れる。closure を実体化（`diff-closures`）する必要は
/// ないため、本関数は eval 由来の 2 マップを比較する。
///
/// 差分種別の意味論:
/// - new のみに在る名前 → [`ChangeKind::Added`]（`old=None`）。
/// - old のみに在る名前 → [`ChangeKind::Removed`]（`new=None`）。
/// - 両方に在り version 文字列が異なる → version 大小比較で [`ChangeKind::Upgraded`] /
///   [`ChangeKind::Downgraded`] を決める。
/// - 両方に在り version が等しい → 更新ではないため差分に含めない。
///
/// version が空文字（eval で `version` 属性が無いパッケージのフォールバック）の扱い: 空 version は
/// `None`（不在）として扱わず空文字のまま比較対象にする。両側空文字で名前のみ存在し続けるなら更新で
/// ないため除外され、片側だけ空（例: 旧版に version が無く新版に在る）なら文字列差として差分に出る。
/// 出力は名前の昇順（`BTreeMap` の反復順）で決定論的に並ぶ。全 delta は [`DeltaSource::NixEval`]。
///
/// caller responsibility: 与える 2 マップは同一参照構成（ci-ref）の old/new lock で eval した宣言
/// パッケージ集合であること。マップ生成（eval プロセス実行・JSON 取得）は adapter が担う。
///
/// `notes_source` の運び方: ノートは**更新後（new）版**のリリースノートを引きたいため、各 delta の
/// `notes_source` は new 側 [`NixPackage`] の値を採る（added/upgraded/downgraded はいずれも new が存在する）。
/// removed は new が無いため `None`。空文字 `notes_source`（`meta.changelog`/`meta.homepage` 不在）は `None`
/// にする（実取得側で取得先無し → version のみへ縮退する）。
pub(crate) fn diff_versions(
    old: &BTreeMap<String, NixPackage>,
    new: &BTreeMap<String, NixPackage>,
) -> Vec<VersionDelta> {
    let mut deltas = Vec::new();

    // new 側を基準に added / upgraded / downgraded / unchanged を判定する（BTreeMap 反復で名前昇順）。
    for (name, new_pkg) in new {
        let repo = empty_to_none(&new_pkg.repo);
        let notes_source = empty_to_none(&new_pkg.notes_source);
        let homepage = empty_to_none(&new_pkg.homepage);
        match old.get(name) {
            // 両側に在る。version 文字列が異なるときだけ差分にする。
            Some(old_pkg) => {
                if old_pkg.version == new_pkg.version {
                    continue;
                }
                let change = compare_versions(&old_pkg.version, &new_pkg.version);
                deltas.push(VersionDelta {
                    name: name.clone(),
                    old: version_value(&old_pkg.version),
                    new: version_value(&new_pkg.version),
                    change,
                    source: DeltaSource::NixEval,
                    repo,
                    notes_source,
                    homepage,
                });
            }
            // new のみに在る → 追加。
            None => deltas.push(VersionDelta {
                name: name.clone(),
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

    // old のみに在る名前 → 削除。
    for (name, old_pkg) in old {
        if !new.contains_key(name) {
            deltas.push(VersionDelta {
                name: name.clone(),
                old: version_value(&old_pkg.version),
                new: None,
                change: ChangeKind::Removed,
                source: DeltaSource::NixEval,
                // removed は new 版が無いためノート取得元も無い。
                repo: None,
                notes_source: None,
                homepage: None,
            });
        }
    }

    deltas
}

/// version 属性が空文字（eval フォールバック）なら `None`、それ以外は版文字列を返す。
///
/// `version` 属性を持たないパッケージ（`pname` だけ、または `parseDrvName` でも版が取れない）は eval で
/// 空文字になる。記録上は版不明として `None`（不在表示）にし、空文字を偽の版として残さない。
fn version_value(version: &str) -> Option<String> {
    if version.is_empty() {
        None
    } else {
        Some(version.to_string())
    }
}

/// 空文字（eval フォールバックで repo/changelog が取れなかった場合）を `None`、それ以外を値として返す。
///
/// 空文字を偽の取得元として運ばないようにする。`repo`/`notes_source` がともに `None` の delta は実取得側で
/// 「取得元不明」として version のみへ縮退する（プラン契約の graceful degradation）。値の妥当性検証
/// （owner/repo 形・host allowlist）は実取得時に行う。
fn empty_to_none(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// 両側に存在する 2 version 文字列を比較し、昇格か降格かを決める。
///
/// 比較規則: ドット/ハイフン区切りの各成分を、数値として解釈できれば数値で、できなければ文字列として
/// 辞書順で比較する（成分位置ごとに数値同士・文字列同士で比べ、種別が混在する位置は数値成分を文字列成分
/// より小さい側に置く＝[`compare_component`] の `(Ok(_), Err(_)) => Less`）。全成分が等しく長さだけ異なる
/// ときは成分数が多い側を新しい
/// とみなす（`1.2` < `1.2.1`）。判定不能（完全に等しい文字列は呼び出し前に除外済み）な場合は既定で
/// [`ChangeKind::Upgraded`] とする。降格は nixpkgs の巻き戻し等で稀に起きるため種別として保持する。
fn compare_versions(old: &str, new: &str) -> ChangeKind {
    match version_ordering(old, new) {
        std::cmp::Ordering::Greater => ChangeKind::Downgraded,
        std::cmp::Ordering::Less => ChangeKind::Upgraded,
        // 文字列としては異なるが成分比較で同順位（例: `1.0` vs `1.0+a` の境界）なら既定で昇格扱い。
        std::cmp::Ordering::Equal => ChangeKind::Upgraded,
    }
}

/// 2 version 文字列の順序を成分単位で比較する。`lhs` を左、`rhs` を右に取り `lhs.cmp(rhs)` 相当を返す。
///
/// 比較は version 差分の意味論（昇格/降格）と Releases API 範囲フィルタ（[`version_in_range`]）の双方が
/// 依拠する domain rule であり、`update_history` module 内で共有する（adapter が同等比較を再実装しない）。
/// 全成分が等しく長さだけ異なるときは成分数が多い側を新しいとみなす（`1.2` < `1.2.1`）。
pub(in crate::update_history) fn version_ordering(lhs: &str, rhs: &str) -> std::cmp::Ordering {
    let lhs_parts = split_components(lhs);
    let rhs_parts = split_components(rhs);
    for (l, r) in lhs_parts.iter().zip(rhs_parts.iter()) {
        let ordering = compare_component(l, r);
        if ordering != std::cmp::Ordering::Equal {
            return ordering;
        }
    }
    lhs_parts.len().cmp(&rhs_parts.len())
}

/// version 文字列を `.` と `-` で成分へ分割する（空成分は除く）。`update_history` module 内で共有する。
pub(in crate::update_history) fn split_components(version: &str) -> Vec<&str> {
    version
        .split(['.', '-'])
        .filter(|s| !s.is_empty())
        .collect()
}

/// 1 成分を比較する。両方が数値解釈できれば数値比較、片方のみ数値なら数値側を小さく、いずれも非数値なら
/// 文字列辞書順で比較する。`update_history` module 内で共有する。
pub(in crate::update_history) fn compare_component(lhs: &str, rhs: &str) -> std::cmp::Ordering {
    match (lhs.parse::<u64>(), rhs.parse::<u64>()) {
        (Ok(l), Ok(r)) => l.cmp(&r),
        (Ok(_), Err(_)) => std::cmp::Ordering::Less,
        (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
        (Err(_), Err(_)) => lhs.cmp(rhs),
    }
}

/// tag/name 文字列から version 様トークンを抽出して正規化する純粋関数（domain rule）。
///
/// GitHub の release tag/name には `v1.2.3` / `1.2.3` / `<pkg>-v1.2.3` / `<pkg>-1.2.3` / `release-1.2.3` の
/// ような揺れがある。`-`/空白/`_` 区切りの各トークンを後ろから見て、最初の文字が数字の version 様トークン
/// （先頭 `v`/`V` は剥がす）を採る。「tag 文字列をどの version へ正規化するか」は外部実装を差し替えても
/// 変わらない整合判定であり domain rule である。version 様トークンが見つからなければ `None`。
pub(in crate::update_history) fn extract_version_token(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed
        .split(['-', ' ', '_'])
        .rev()
        .map(normalize_version)
        .find(|token| token.chars().next().is_some_and(|c| c.is_ascii_digit()))
}

/// version 文字列を正規化する純粋関数（先頭の `v`/`V` を剥がし、前後空白を除く）。domain rule。
///
/// tag 揺れ（`v{ver}` と `{ver}`）を同一 version として扱うための正規化であり、範囲判定・比較の前段に置く。
pub(in crate::update_history) fn normalize_version(version: &str) -> String {
    let trimmed = version.trim();
    trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed)
        .to_string()
}

/// release tag（無ければ name）から version 文字列を抽出して正規化する純粋関数（domain rule）。
///
/// tag を優先し、tag から version 様トークンが取れなければ name から抽出する（[`extract_version_token`]）。
/// いずれからも取れなければ `None`。Releases API のどのリリースが範囲に属するかを決める基準値であり、
/// 外部実装に依らない値抽出規則として domain に置く。
pub(in crate::update_history) fn release_version(tag: &str, name: &str) -> Option<String> {
    extract_version_token(tag).or_else(|| extract_version_token(name))
}

/// 抽出済み release version が `(old, new]`（old 排他・new 包含）に入るかを判定する純粋関数（domain rule）。
///
/// 「どのリリースが old→new の更新範囲に属するか」は外部取得実装（Releases API 等）を差し替えても変わらない
/// 整合判定であり domain rule である。`version` は抽出済みの正規化対象 version（[`release_version`] の戻り値）。
/// `old`/`new` は版文字列（`v` 接頭等の揺れを含みうる）で、`None` のときその側の境界を課さない（old=None なら
/// 下限なし、new=None なら上限なし）。比較は [`version_ordering`] で行い、境界値は [`normalize_version`] で
/// tag 揺れを正規化してから比べる。
pub(in crate::update_history) fn version_in_range(
    version: &str,
    old: Option<&str>,
    new: Option<&str>,
) -> bool {
    // old 排他: version > old（old があるときだけ下限を課す）。
    if let Some(old) = old.map(normalize_version)
        && version_ordering(version, &old) != std::cmp::Ordering::Greater
    {
        return false;
    }
    // new 包含: version <= new（new があるときだけ上限を課す）。
    if let Some(new) = new.map(normalize_version)
        && version_ordering(version, &new) == std::cmp::Ordering::Greater
    {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    //! eval マップ比較の種別確定（added/removed/upgraded/downgraded/unchanged 除外・version 欠落
    //! フォールバック）とマージ順序を固定する。

    use super::*;

    /// name→version のペアを `NixPackage` マップ（repo/notes_source 空）へ畳む。
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

    /// name→(version, repo, notes_source) を `NixPackage` マップへ畳む（repo/notes_source 運搬テスト用）。
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

    /// 名前で delta を引く。見つからなければテスト失敗（`expect` を使わず `Option` で照合する）。
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
        // 名前昇順: neovim(upgraded), ripgrep(added), ruby(downgraded), oldpkg(removed は末尾)。
        assert_eq!(deltas.len(), 4);

        assert_eq!(
            find(&deltas, "neovim"),
            Some(&VersionDelta {
                name: "neovim".to_string(),
                old: Some("0.10.2".to_string()),
                new: Some("0.11.0".to_string()),
                change: ChangeKind::Upgraded,
                source: DeltaSource::NixEval,
                repo: None,
                notes_source: None,
                homepage: None,
            })
        );
        assert_eq!(
            find(&deltas, "ripgrep"),
            Some(&VersionDelta {
                name: "ripgrep".to_string(),
                old: None,
                new: Some("14.1.0".to_string()),
                change: ChangeKind::Added,
                source: DeltaSource::NixEval,
                repo: None,
                notes_source: None,
                homepage: None,
            })
        );
        assert_eq!(
            find(&deltas, "ruby"),
            Some(&VersionDelta {
                name: "ruby".to_string(),
                old: Some("3.4.0".to_string()),
                new: Some("3.3.10".to_string()),
                change: ChangeKind::Downgraded,
                source: DeltaSource::NixEval,
                repo: None,
                notes_source: None,
                homepage: None,
            })
        );
        assert_eq!(
            find(&deltas, "oldpkg"),
            Some(&VersionDelta {
                name: "oldpkg".to_string(),
                old: Some("1.0.0".to_string()),
                new: None,
                change: ChangeKind::Removed,
                source: DeltaSource::NixEval,
                repo: None,
                notes_source: None,
                homepage: None,
            })
        );
    }

    #[test]
    fn diff_excludes_unchanged_versions() {
        let old = map(&[("zlib", "1.3.1"), ("git", "2.53.0")]);
        let new = map(&[("zlib", "1.3.1"), ("git", "2.54.0")]);
        let deltas = diff_versions(&old, &new);
        // 版が変わった git だけが残り、unchanged zlib は除外される。
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].name, "git");
        assert!(
            !deltas.iter().any(|d| d.name == "zlib"),
            "unchanged は除外する: {deltas:?}"
        );
    }

    #[test]
    fn diff_treats_empty_version_as_absent() {
        // eval で version 属性を持たないパッケージは空文字になる。両側空文字なら更新でなく除外、
        // 片側だけ空（旧版に版が無く新版に在る）なら追加相当の差分として old=None で出る。
        let old = map(&[("google-cloud-sdk", ""), ("python3", "")]);
        let new = map(&[("google-cloud-sdk", "500.0.0"), ("python3", "")]);
        let deltas = diff_versions(&old, &new);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].name, "google-cloud-sdk");
        // 旧版が空文字（版不明）なので old は None、new は版文字列。
        assert_eq!(deltas[0].old, None);
        assert_eq!(deltas[0].new.as_deref(), Some("500.0.0"));
    }

    #[test]
    fn diff_output_is_name_sorted_for_determinism() {
        let old = map(&[("a", "1"), ("c", "1")]);
        let new = map(&[("a", "2"), ("b", "1"), ("c", "2")]);
        let deltas = diff_versions(&old, &new);
        let names: Vec<&str> = deltas.iter().map(|d| d.name.as_str()).collect();
        // new 由来（a,b,c）が名前昇順、removed は無し。
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn diff_handles_multi_component_versions() {
        let old = map(&[("pkg", "1.2"), ("nodejs", "22.22.2")]);
        let new = map(&[("pkg", "1.2.1"), ("nodejs", "22.22.10")]);
        let deltas = diff_versions(&old, &new);
        // 1.2 < 1.2.1（成分数）、22.22.2 < 22.22.10（数値成分）でいずれも upgraded。
        assert!(deltas.iter().all(|d| d.change == ChangeKind::Upgraded));
    }

    #[test]
    fn diff_carries_new_side_repo_and_notes_source_into_delta() {
        // nix eval 由来 delta は更新後（new）版の repo（owner/repo）と changelog 取得元を運ぶ。
        // upgraded/added は new の repo/notes_source を採り、removed は new が無いため両方 None。
        // 空文字（repo/changelog 不在）は None へ縮退する。
        let old = map_with_notes(&[
            (
                "neovim",
                "0.10",
                "neovim/neovim",
                "https://github.com/neovim/neovim/blob/master/CHANGELOG",
            ),
            ("oldpkg", "1.0", "", "https://example.com/old"),
        ]);
        let new = map_with_notes(&[
            (
                "neovim",
                "0.11",
                "neovim/neovim",
                "https://github.com/neovim/neovim/blob/master/CHANGELOG",
            ),
            (
                "ripgrep",
                "14.1",
                "BurntSushi/ripgrep",
                "https://github.com/BurntSushi/ripgrep",
            ),
            ("nonotes", "2.0", "", ""),
        ]);
        let deltas = diff_versions(&old, &new);

        // upgraded: new 側の repo と notes_source を運ぶ。
        assert_eq!(
            find(&deltas, "neovim").and_then(|d| d.repo.as_deref()),
            Some("neovim/neovim")
        );
        assert_eq!(
            find(&deltas, "neovim").and_then(|d| d.notes_source.as_deref()),
            Some("https://github.com/neovim/neovim/blob/master/CHANGELOG")
        );
        // added: new 側の repo を運ぶ。
        assert_eq!(
            find(&deltas, "ripgrep").and_then(|d| d.repo.as_deref()),
            Some("BurntSushi/ripgrep")
        );
        // repo/changelog 不在（空文字）は None へ縮退。
        assert_eq!(find(&deltas, "nonotes").map(|d| d.repo.clone()), Some(None));
        assert_eq!(
            find(&deltas, "nonotes").map(|d| d.notes_source.clone()),
            Some(None)
        );
        // removed: new 版が無いため repo/notes_source は None。
        assert_eq!(find(&deltas, "oldpkg").map(|d| d.repo.clone()), Some(None));
        assert_eq!(
            find(&deltas, "oldpkg").map(|d| d.notes_source.clone()),
            Some(None)
        );
    }

    #[test]
    fn normalize_version_strips_v_prefix_and_whitespace() {
        assert_eq!(normalize_version("v1.2.3"), "1.2.3");
        assert_eq!(normalize_version("V2.0"), "2.0");
        assert_eq!(normalize_version("  1.0  "), "1.0");
        // 先頭が数字なら剥がさない。
        assert_eq!(normalize_version("3.4.0"), "3.4.0");
    }

    #[test]
    fn extract_version_token_picks_last_version_like_token() {
        // tag 揺れ（`v` 接頭・接頭辞付き・接頭なし）から version 様トークンを採り正規化する。
        assert_eq!(extract_version_token("v1.2.3").as_deref(), Some("1.2.3"));
        assert_eq!(extract_version_token("1.2.3").as_deref(), Some("1.2.3"));
        assert_eq!(
            extract_version_token("pkg-v1.5.0").as_deref(),
            Some("1.5.0")
        );
        assert_eq!(extract_version_token("release-1.0").as_deref(), Some("1.0"));
        // version 様トークンが無ければ None。
        assert_eq!(extract_version_token("latest"), None);
        assert_eq!(extract_version_token("   "), None);
    }

    #[test]
    fn release_version_prefers_tag_then_name() {
        assert_eq!(
            release_version("v1.2.3", "anything").as_deref(),
            Some("1.2.3")
        );
        // tag から取れなければ name から抽出する。
        assert_eq!(
            release_version("latest", "Release 2.0.0").as_deref(),
            Some("2.0.0")
        );
        // いずれからも取れなければ None。
        assert_eq!(release_version("latest", "nightly"), None);
    }

    #[test]
    fn version_in_range_is_old_exclusive_new_inclusive() {
        // (old, new] = (1.0.0, 1.2.0]: old は排他、new は包含、範囲外は除外。
        let old = Some("1.0.0");
        let new = Some("1.2.0");
        assert!(!version_in_range("1.0.0", old, new), "old は排他");
        assert!(version_in_range("1.1.0", old, new), "範囲内");
        assert!(version_in_range("1.2.0", old, new), "new は包含");
        assert!(!version_in_range("0.9.0", old, new), "old 未満は除外");
        assert!(!version_in_range("1.3.0", old, new), "new 超過は除外");
    }

    #[test]
    fn version_in_range_normalizes_boundary_tag_variants() {
        // 境界値が `v` 接頭の tag 揺れでも正規化して比較する。
        assert!(version_in_range("1.5.0", Some("v1.0.0"), Some("v2.0.0")));
        assert!(!version_in_range("2.5.0", Some("v1.0.0"), Some("v2.0.0")));
    }

    #[test]
    fn version_in_range_with_unbounded_old_or_new() {
        // old=None なら下限なし、new=None なら上限なし、両方 None なら常に範囲内。
        assert!(version_in_range("0.1.0", None, Some("1.0.0")));
        assert!(version_in_range("9.9.9", Some("1.0.0"), None));
        assert!(version_in_range("5.0.0", None, None));
    }

    #[test]
    fn merge_keeps_nix_first_then_brew_preserving_each_order() {
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
        assert_eq!(merged[0].name, "neovim");
        assert_eq!(merged[0].source, DeltaSource::NixEval);
        assert_eq!(merged[1].name, "firefox");
        assert_eq!(merged[1].source, DeltaSource::BrewTap);
    }
}
