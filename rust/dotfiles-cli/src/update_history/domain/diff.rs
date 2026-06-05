//! `nix store diff-closures` 出力テキストを構造化 version 差分へ変換する純粋パーサと、
//! brew tap rev 版差分の表現型。
//!
//! ここに置くのは外部 I/O を持たない純粋な文字列パースとマージだけである。nix プロセス実行
//! （`adapters/nix.rs`）、tap rev からの formula/cask 版差分読み取り（`adapters/brew.rs`）は adapter の
//! 責務であり、本 module は取得済みテキスト/値を構造へ翻訳する規則だけを domain rule として固定する。

use super::wire::ChangeKind;
use crate::Result;

/// nix が version 不在を示すために使う記号（`∅`）。
const ABSENT: &str = "∅";
/// old/new を区切る矢印。
const ARROW: &str = "→";

/// 差分 version の出所（nix クロージャか Homebrew tap rev か）。
///
/// nix=`diff-closures` と brew=tap rev 版差分は同じ version 差分モデルへ統合されるが、出所により
/// ノート取得先（forge releases / cask homepage）が変わるため、出所だけは型として保持する。
/// 実取得は adapter（`adapters/nix.rs`・`adapters/brew.rs`）が行い、本 module は出所タグ付けまでを担う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeltaSource {
    /// `nix store diff-closures` 由来。
    NixClosure,
    /// Homebrew tap rev の formula/cask ファイル差分由来。
    BrewTap,
}

/// 単一パッケージの version 差分（パーサ/マージの中間表現）。
///
/// `old` / `new` は version が不在のとき `None`。`change` は ∅ の位置から確定する種別であり、
/// 両側存在時は version 文字列の意味解釈を domain へ持ち込まないため既定で `Upgraded` とする
/// （降格判定が必要なら上位で version 比較規則を別途与える）。`source` は nix/brew いずれの
/// 差分系統かを示し、両系統を同一モデルへマージしてもノート取得先を区別できるようにする。
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
    /// 差分の出所（nix クロージャ / brew tap）。
    pub(crate) source: DeltaSource,
}

/// nix=`diff-closures` と brew=tap rev の version 差分を同一モデルへ統合する。
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

/// `nix store diff-closures` の出力テキストを [`VersionDelta`] 列へ変換する純粋パーサ。
///
/// 各行の形式は `name: <old> → <new>[, <size delta>]` を基本とする。`<old>` / `<new>` は
/// `∅`（不在）またはカンマ区切り version 列であり、複数 version は安定のため `, ` で連結して保持する。
/// 末尾の size delta（`, +1.0 KiB` 等）は version 列と矢印で区切れないため、矢印の右側を最初の
/// `,` で切って version 部分だけを採る。`∅ → x` は `Added`、`x → ∅` は `Removed`、それ以外は
/// `Upgraded`。矢印を含まない行（ヘッダ・空行・size 集計行）は version 差分でないため無視する。
///
/// caller responsibility: 行内に矢印があり name が空でないことだけを差分行の条件とする。形式が
/// 想定外（矢印が複数、name 欠落）の行はパース失敗として `Err` を返し、部分的に壊れた記録を作らない。
pub(crate) fn parse_diff_closures(text: &str) -> Result<Vec<VersionDelta>> {
    let mut deltas = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || !line.contains(ARROW) {
            continue;
        }
        deltas.push(parse_line(line)?);
    }
    Ok(deltas)
}

/// 矢印を含む 1 行を [`VersionDelta`] に変換する。
fn parse_line(line: &str) -> Result<VersionDelta> {
    let Some((name_part, rest)) = line.split_once(':') else {
        anyhow::bail!("diff-closures line has no package name: {line}");
    };
    let name = name_part.trim();
    if name.is_empty() {
        anyhow::bail!("diff-closures line has empty package name: {line}");
    }

    let mut sides = rest.split(ARROW);
    let Some(old_raw) = sides.next() else {
        anyhow::bail!("diff-closures line missing old version: {line}");
    };
    let Some(new_raw) = sides.next() else {
        anyhow::bail!("diff-closures line missing new version: {line}");
    };
    if sides.next().is_some() {
        anyhow::bail!("diff-closures line has multiple arrows: {line}");
    }

    let old = parse_versions(old_raw);
    let new = parse_versions(new_raw);
    let change = match (&old, &new) {
        (None, Some(_)) => ChangeKind::Added,
        (Some(_), None) => ChangeKind::Removed,
        _ => ChangeKind::Upgraded,
    };

    Ok(VersionDelta {
        name: name.to_string(),
        old,
        new,
        change,
        source: DeltaSource::NixClosure,
    })
}

/// 矢印の片側テキストから version 文字列を抽出する。
///
/// 末尾の size delta を落とすため最初の `,` 以降は無視せず——nixpkgs は version 列自体を `, ` で
/// 区切るため——`, ` で分割した各要素のうち、size delta（先頭が `+`/`-` で `KiB`/`MiB`/`B` を含む）を
/// 除いた version だけを `, ` で連結する。すべて size delta か `∅` のみなら `None`。
fn parse_versions(side: &str) -> Option<String> {
    let trimmed = side.trim();
    if trimmed.is_empty() || trimmed == ABSENT {
        return None;
    }
    let versions: Vec<&str> = trimmed
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty() && *token != ABSENT && !is_size_delta(token))
        .collect();
    if versions.is_empty() {
        None
    } else {
        Some(versions.join(", "))
    }
}

/// `+1.0 KiB` / `-512.0 B` のような size delta token かを判定する。
fn is_size_delta(token: &str) -> bool {
    let starts_with_sign = token.starts_with('+') || token.starts_with('-');
    let has_unit = token.ends_with("B") || token.ends_with("KiB") || token.ends_with("MiB");
    starts_with_sign && has_unit
}

#[cfg(test)]
mod tests {
    //! diff-closures パースの種別確定・size delta 除去・複数 version 保持・異常行扱いを固定する。

    use super::*;

    #[test]
    fn parses_upgraded_added_removed_lines() -> Result<()> {
        let text = "\
neovim: 0.10.2 → 0.11.0, +1.2 KiB
ripgrep: ∅ → 14.1.0, +3.0 MiB
oldpkg: 1.0.0 → ∅, -2.0 KiB
";
        let deltas = parse_diff_closures(text)?;
        assert_eq!(deltas.len(), 3);

        assert_eq!(deltas[0].name, "neovim");
        assert_eq!(deltas[0].old.as_deref(), Some("0.10.2"));
        assert_eq!(deltas[0].new.as_deref(), Some("0.11.0"));
        assert_eq!(deltas[0].change, ChangeKind::Upgraded);

        assert_eq!(deltas[1].name, "ripgrep");
        assert_eq!(deltas[1].old, None);
        assert_eq!(deltas[1].new.as_deref(), Some("14.1.0"));
        assert_eq!(deltas[1].change, ChangeKind::Added);

        assert_eq!(deltas[2].name, "oldpkg");
        assert_eq!(deltas[2].old.as_deref(), Some("1.0.0"));
        assert_eq!(deltas[2].new, None);
        assert_eq!(deltas[2].change, ChangeKind::Removed);
        Ok(())
    }

    #[test]
    fn keeps_multiple_versions_and_ignores_non_diff_lines() -> Result<()> {
        let text = "\
Version changes:
glibc: 2.38, 2.39 → 2.40
Closure size: 100.0 MiB → 101.0 MiB
";
        let deltas = parse_diff_closures(text)?;
        // \"Version changes:\" と \"Closure size: ... → ...\" のうち、後者は version でなく size 集計だが
        // 矢印を含むためパース対象になる。size token は除去され、version が残らないので old/new とも None。
        assert_eq!(deltas.len(), 2);

        assert_eq!(deltas[0].name, "glibc");
        assert_eq!(deltas[0].old.as_deref(), Some("2.38, 2.39"));
        assert_eq!(deltas[0].new.as_deref(), Some("2.40"));
        Ok(())
    }

    #[test]
    fn rejects_line_with_multiple_arrows() {
        let result = parse_diff_closures("weird: 1.0 → 2.0 → 3.0\n");
        assert!(result.is_err());
    }

    #[test]
    fn skips_lines_without_arrow() -> Result<()> {
        let deltas = parse_diff_closures("no arrow here\n\n")?;
        assert!(deltas.is_empty());
        Ok(())
    }

    #[test]
    fn parsed_deltas_are_tagged_as_nix_closure_source() -> Result<()> {
        let deltas = parse_diff_closures("neovim: 0.10.2 → 0.11.0\n")?;
        assert_eq!(deltas[0].source, DeltaSource::NixClosure);
        Ok(())
    }

    #[test]
    fn merge_keeps_nix_first_then_brew_preserving_each_order() {
        let nix = vec![VersionDelta {
            name: "neovim".to_string(),
            old: Some("0.10".to_string()),
            new: Some("0.11".to_string()),
            change: ChangeKind::Upgraded,
            source: DeltaSource::NixClosure,
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
        assert_eq!(merged[0].source, DeltaSource::NixClosure);
        assert_eq!(merged[1].name, "firefox");
        assert_eq!(merged[1].source, DeltaSource::BrewTap);
    }
}
