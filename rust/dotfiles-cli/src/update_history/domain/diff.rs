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

/// 単一パッケージの version 差分（比較/マージの中間表現）。
///
/// `old` / `new` は version が不在のとき `None`。`change` は両側の存在有無と version 文字列の
/// 大小比較から確定する種別である。`source` は nix/brew いずれの差分系統かを示し、両系統を同一
/// モデルへマージしてもノート取得先を区別できるようにする。
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
pub(crate) fn diff_versions(
    old: &BTreeMap<String, String>,
    new: &BTreeMap<String, String>,
) -> Vec<VersionDelta> {
    let mut deltas = Vec::new();

    // new 側を基準に added / upgraded / downgraded / unchanged を判定する（BTreeMap 反復で名前昇順）。
    for (name, new_version) in new {
        match old.get(name) {
            // 両側に在る。version 文字列が異なるときだけ差分にする。
            Some(old_version) => {
                if old_version == new_version {
                    continue;
                }
                let change = compare_versions(old_version, new_version);
                deltas.push(VersionDelta {
                    name: name.clone(),
                    old: version_value(old_version),
                    new: version_value(new_version),
                    change,
                    source: DeltaSource::NixEval,
                });
            }
            // new のみに在る → 追加。
            None => deltas.push(VersionDelta {
                name: name.clone(),
                old: None,
                new: version_value(new_version),
                change: ChangeKind::Added,
                source: DeltaSource::NixEval,
            }),
        }
    }

    // old のみに在る名前 → 削除。
    for (name, old_version) in old {
        if !new.contains_key(name) {
            deltas.push(VersionDelta {
                name: name.clone(),
                old: version_value(old_version),
                new: None,
                change: ChangeKind::Removed,
                source: DeltaSource::NixEval,
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

/// 両側に存在する 2 version 文字列を比較し、昇格か降格かを決める。
///
/// 比較規則: ドット/ハイフン区切りの各成分を、数値として解釈できれば数値で、できなければ文字列として
/// 辞書順で比較する（数値成分は文字列より小さいとみなさず、成分位置ごとに数値同士・文字列同士で比べ、
/// 種別が混在する位置は数値を小さい側に置く）。全成分が等しく長さだけ異なるときは成分数が多い側を新しい
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

/// 2 version 文字列の順序を成分単位で比較する。`old` を左、`new` を右に取り `old.cmp(new)` 相当を返す。
fn version_ordering(old: &str, new: &str) -> std::cmp::Ordering {
    let old_parts = split_components(old);
    let new_parts = split_components(new);
    for (lhs, rhs) in old_parts.iter().zip(new_parts.iter()) {
        let ordering = compare_component(lhs, rhs);
        if ordering != std::cmp::Ordering::Equal {
            return ordering;
        }
    }
    old_parts.len().cmp(&new_parts.len())
}

/// version 文字列を `.` と `-` で成分へ分割する。
fn split_components(version: &str) -> Vec<&str> {
    version
        .split(['.', '-'])
        .filter(|s| !s.is_empty())
        .collect()
}

/// 1 成分を比較する。両方が数値解釈できれば数値比較、片方のみ数値なら数値側を小さく、いずれも非数値なら
/// 文字列辞書順で比較する。
fn compare_component(lhs: &str, rhs: &str) -> std::cmp::Ordering {
    match (lhs.parse::<u64>(), rhs.parse::<u64>()) {
        (Ok(l), Ok(r)) => l.cmp(&r),
        (Ok(_), Err(_)) => std::cmp::Ordering::Less,
        (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
        (Err(_), Err(_)) => lhs.cmp(rhs),
    }
}

#[cfg(test)]
mod tests {
    //! eval マップ比較の種別確定（added/removed/upgraded/downgraded/unchanged 除外・version 欠落
    //! フォールバック）とマージ順序を固定する。

    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, version)| ((*name).to_string(), (*version).to_string()))
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
    fn merge_keeps_nix_first_then_brew_preserving_each_order() {
        let nix = vec![VersionDelta {
            name: "neovim".to_string(),
            old: Some("0.10".to_string()),
            new: Some("0.11".to_string()),
            change: ChangeKind::Upgraded,
            source: DeltaSource::NixEval,
        }];
        let brew = vec![VersionDelta {
            name: "firefox".to_string(),
            old: Some("120".to_string()),
            new: Some("121".to_string()),
            change: ChangeKind::Upgraded,
            source: DeltaSource::BrewTap,
        }];

        let merged = merge_version_deltas(nix, brew);

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].name, "neovim");
        assert_eq!(merged[0].source, DeltaSource::NixEval);
        assert_eq!(merged[1].name, "firefox");
        assert_eq!(merged[1].source, DeltaSource::BrewTap);
    }
}
