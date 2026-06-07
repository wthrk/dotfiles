//! version 差分と構造化変更リストから記録用 [`UpdateEntry`] を組み立てる domain 規則。
//!
//! `record` use case は diff（nix/brew）→ ノート取得 → LLM 抽出 → サニタイズの順で素材を集めるが、
//! 「version 差分 + 変更リスト + URL を 1 パッケージ更新へ対応づけ、エントリ全体の severity / overall を
//! 算出する」のは外部実装を差し替えても変わらない業務規則であり domain に置く。組み立て手段（port 呼び出し
//! 順序）は application、URL の host 妥当性は [`super::validate`]、severity 算出は [`super::severity`] が担う。

use super::diff::{DeltaSource, VersionDelta};
use super::severity::{overall_headline, severity_of};
use super::wire::{ChangeItem, PackageUpdate, Severity, UpdateEntry};

/// 1 パッケージ分の素材（version 差分 + 変更リスト + ノート URL）を表す中間入力。
///
/// `delta` は nix/brew いずれかの version 差分、`change_items` はサニタイズ済み構造化変更、
/// `notes_url` はサニタイズ済みノート URL（許可ホスト https のみ／取得不能なら `None`）。
/// application はこの素材を delta ごとに 1 件ずつ用意し、[`build_entry`] へ渡す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageMaterial {
    /// 対象パッケージの version 差分。
    pub(crate) delta: VersionDelta,
    /// サニタイズ済み構造化変更リスト（空なら概要なし）。
    pub(crate) change_items: Vec<ChangeItem>,
    /// サニタイズ済みノート URL（許可ホスト https のみ）。
    pub(crate) notes_url: Option<String>,
}

/// version 差分 1 件を記録用 [`PackageUpdate`] へ変換する。
///
/// `declared` は差分の出所（[`DeltaSource`]）で決める。`nix eval` 由来の delta は ci-ref の
/// `home.packages` + `environment.systemPackages` の**宣言パッケージ**であり、利用者が宣言した実アプリ
/// なので `declared: true`（既定表示）にする。brew cask 由来の delta も宣言した実アプリ（`homebrew.nix`
/// の cask）なので `declared: true`。`old`/`new`/`change` は差分の値をそのまま採る。
///
/// 補足: eval ベース化以前は `nix store diff-closures` がクロージャの推移的（低レベル）依存まで含むため
/// nix 由来を `declared: false` で畳んでいたが、eval は宣言パッケージ集合だけを返し推移的依存を含まない
/// ため、nix 由来も宣言アプリとして既定表示にする。
fn to_package_update(material: PackageMaterial) -> PackageUpdate {
    let declared = match material.delta.source {
        // nix eval 差分は宣言パッケージのみ（推移的依存を含まない）なので既定表示する。
        DeltaSource::NixEval => true,
        // brew cask は宣言した実アプリなので既定表示する。
        DeltaSource::BrewTap => true,
    };
    PackageUpdate {
        name: material.delta.name,
        old: material.delta.old,
        new: material.delta.new,
        change: material.delta.change,
        declared,
        notes_url: material.notes_url,
        change_items: material.change_items,
    }
}

/// パッケージ素材列から、severity / overall を機械算出した 1 件の [`UpdateEntry`] を組み立てる。
///
/// `severity` と `overall` は全パッケージの change_item を平坦化した集合から [`super::severity`] の
/// 単一関数で算出する（`record` と `show` で同一規則を共有し、二重実装しない）。`at` / nixpkgs rev /
/// `reference` は呼び出し側（CI 注入値）をそのまま記録する。本関数は素材を wire 表現へ確定するだけで、
/// ファイル追記や catch-up 集約は行わない。
pub(crate) fn build_entry(
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

/// 既に集約・選択済みの change_item 集合から severity を再算出する（`show` 共有口）。
///
/// catch-up 集約後の表示で severity バッジを出すため、[`super::severity::severity_of`] を再公開する
/// 薄い委譲ではなく、show が application から 1 か所で呼ぶための再算出口として置く。
pub(crate) fn recompute_severity(items: &[ChangeItem]) -> Severity {
    severity_of(items)
}

#[cfg(test)]
mod tests {
    //! version 差分 + 変更リストから severity/overall を機械算出した記録エントリ組み立てを固定する。

    use super::*;
    use crate::update_history::domain::diff::{DeltaSource, VersionDelta};
    use crate::update_history::domain::wire::{ChangeCategory, ChangeItem, ChangeKind, Severity};

    fn delta(name: &str) -> VersionDelta {
        delta_with_source(name, DeltaSource::NixEval)
    }

    fn delta_with_source(name: &str, source: DeltaSource) -> VersionDelta {
        VersionDelta {
            name: name.to_string(),
            old: Some("1.0".to_string()),
            new: Some("1.1".to_string()),
            change: ChangeKind::Upgraded,
            source,
        }
    }

    fn item(category: ChangeCategory) -> ChangeItem {
        ChangeItem {
            category,
            text: "変更".to_string(),
            ref_url: None,
        }
    }

    #[test]
    fn build_entry_computes_severity_and_overall_from_all_items() {
        let materials = vec![
            PackageMaterial {
                delta: delta("openssl"),
                change_items: vec![item(ChangeCategory::Security)],
                notes_url: Some("https://github.com/openssl/openssl".to_string()),
            },
            PackageMaterial {
                delta: delta("neovim"),
                change_items: vec![item(ChangeCategory::Feature)],
                notes_url: None,
            },
        ];

        let entry = build_entry(
            "2026-06-05T18:00:11Z".to_string(),
            "old".to_string(),
            "new".to_string(),
            "darwinConfigurations.ci".to_string(),
            materials,
        );

        assert_eq!(entry.packages.len(), 2);
        assert_eq!(entry.severity, Severity::Critical);
        assert_eq!(entry.overall, "2アプリ更新: 🔒1 ✨1");
        // eval ベース化後: nix eval 由来は宣言パッケージなので declared=true。
        assert!(entry.packages[0].declared);
    }

    #[test]
    fn declared_is_true_for_nix_eval_and_brew_cask() {
        // eval ベース化後の退行固定: `nix eval` 由来は ci-ref の宣言パッケージ（home.packages +
        // systemPackages）であり推移的依存を含まないため declared=true（既定表示）。brew cask 由来も
        // 宣言した実アプリなので declared=true。出所（DeltaSource）で判別する。
        let materials = vec![
            PackageMaterial {
                delta: delta_with_source("neovim", DeltaSource::NixEval),
                change_items: Vec::new(),
                notes_url: None,
            },
            PackageMaterial {
                delta: delta_with_source("firefox", DeltaSource::BrewTap),
                change_items: Vec::new(),
                notes_url: None,
            },
        ];
        let entry = build_entry(
            "at".to_string(),
            "o".to_string(),
            "n".to_string(),
            "ref".to_string(),
            materials,
        );
        assert_eq!(entry.packages[0].name, "neovim");
        assert!(entry.packages[0].declared, "nix eval 由来は declared=true");
        assert_eq!(entry.packages[1].name, "firefox");
        assert!(entry.packages[1].declared, "brew cask 由来は declared=true");
    }

    #[test]
    fn build_entry_without_change_items_is_none_severity() {
        let materials = vec![PackageMaterial {
            delta: delta("zlib"),
            change_items: Vec::new(),
            notes_url: None,
        }];
        let entry = build_entry(
            "at".to_string(),
            "o".to_string(),
            "n".to_string(),
            "ref".to_string(),
            materials,
        );
        assert_eq!(entry.severity, Severity::None);
        assert_eq!(entry.overall, "1アプリ更新");
    }
}
