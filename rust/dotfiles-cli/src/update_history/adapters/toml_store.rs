//! `HistoryStorePort` を `docs/update-history/<YYYY-MM>.toml` のファイル I/O へ接続する adapter。
//!
//! 1 ファイルに複数の `[[update]]` を持つ TOML を read/append する。read は不存在なら空 Vec を返し、
//! append は既存エントリを読み出してから新エントリを末尾に足し、全体を直列化して書き戻す（1 ファイル
//! 複数件・追記）。serde derive を介した TOML encode/decode の具体実装はこの adapter に閉じ、domain の
//! wire 型は `toml` クレートへ依存しない。catch-up のチェーン連結・表示時集約・範囲選択は domain/application
//! の責務であり、本 adapter は単純な永続化境界（read 全件 / append 1 件）に限定する。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::update_history::domain::wire::UpdateEntry;
use crate::update_history::ports::HistoryStorePort;

/// 履歴 TOML ファイル全体の wire 表現（`[[update]]` の列）。encode/decode 専用で adapter に閉じる。
#[derive(Default, Serialize, Deserialize)]
struct HistoryDocument {
    #[serde(default, rename = "update")]
    updates: Vec<UpdateEntry>,
}

/// 単一履歴ファイルへの read/append を `HistoryStorePort` 契約へ翻訳する adapter。
pub(in crate::update_history) struct TomlHistoryStoreAdapter {
    /// 対象 `docs/update-history/<YYYY-MM>.toml` の絶対 or 相対パス。
    path: PathBuf,
}

impl TomlHistoryStoreAdapter {
    /// 対象履歴ファイルパスを束ねた adapter を作る。
    pub(in crate::update_history) fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// 既存ファイルを読み、document を返す（不存在なら空 document）。
    fn read_document(path: &Path) -> Result<HistoryDocument> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(toml::from_str(&text)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(HistoryDocument::default())
            }
            Err(error) => Err(error.into()),
        }
    }

    /// directory 配下の **月次履歴ファイル（`<YYYY-MM>.toml`）だけ** を名前順に読み、エントリを連結する。
    ///
    /// show の既定 source は月次ファイルが並ぶ `docs/update-history` directory であり、ファイル名（`<YYYY-MM>`）
    /// の辞書順 = 時系列順になるため、名前順 sort で記録順（最古→最新）に揃える。directory 不存在なら空。
    ///
    /// **registry ファイルの除外（finding 3369076725）**: ノート取得元レジストリ `notes-sources.toml` も同じ
    /// `docs/update-history/` directory に置かれる。拡張子 `.toml` だけで拾うと registry も `HistoryDocument`
    /// として読んでしまい、registry がパッケージ名 `update` を学習して `[update]` テーブルを持つと
    /// `update = Vec<UpdateEntry>` として誤デシリアライズされ、show / 適用後要約が丸ごと失敗する。これを防ぐため、
    /// **ファイル名が `<YYYY-MM>.toml` 形（[`is_monthly_history_file`]）のものだけ**を履歴として読み、registry や
    /// 将来追加され得る他の非履歴 TOML を構造的に除外する（拡張子フィルタでなく履歴ファイル名規則で限定する）。
    fn read_directory(path: &Path) -> Result<Vec<UpdateEntry>> {
        let mut files: Vec<PathBuf> = match std::fs::read_dir(path) {
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
            entries.extend(Self::read_document(&file)?.updates);
        }
        Ok(entries)
    }
}

/// ファイル名が月次履歴ファイル `<YYYY-MM>.toml` 形かを判定する純粋関数。
///
/// 履歴 directory には月次履歴ファイルとノート取得元レジストリ `notes-sources.toml` が混在するため、履歴
/// として連結読みするファイルを「4 桁年 `-` 2 桁月 `.toml`」の固定形に限定する（finding 3369076725）。これにより
/// registry（`notes-sources.toml`）や将来の非履歴 TOML を構造的に除外する。`YYYY`/`MM` は ASCII 数字のみ許容し、
/// 値域（月 01–12 等）までは検査しない（履歴ファイルの命名規則一致だけを担い、内容妥当性は読取り側に委ねる）。
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

impl HistoryStorePort for TomlHistoryStoreAdapter {
    fn read_entries(&self) -> Result<Vec<UpdateEntry>> {
        // show は月次ファイルが並ぶ directory を、record は単一の `<YYYY-MM>.toml` を指す。
        // directory なら全月次を連結し、file なら 1 ファイルを読む。
        if self.path.is_dir() {
            Self::read_directory(&self.path)
        } else {
            Ok(Self::read_document(&self.path)?.updates)
        }
    }

    fn append_entry(&self, entry: &UpdateEntry) -> Result<()> {
        let mut document = Self::read_document(&self.path)?;
        document.updates.push(entry.clone());
        // 追記先 directory（`docs/update-history`）が無い初回でも書けるよう、親 directory を確保する。
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let rendered = toml::to_string(&document)?;
        std::fs::write(&self.path, rendered)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! read（不存在で空）と append（既存保持・複数件）の往復をテンポラリファイルで固定する。

    use super::TomlHistoryStoreAdapter;
    use crate::update_history::domain::wire::{
        ChangeKind, PackageSource, PackageUpdate, Severity, UpdateEntry,
    };
    use crate::update_history::ports::HistoryStorePort;

    fn sample(at: &str, name: &str) -> UpdateEntry {
        UpdateEntry {
            at: at.to_string(),
            nixpkgs_old: "o".to_string(),
            nixpkgs_new: "n".to_string(),
            reference: "darwinConfigurations.ci".to_string(),
            severity: Severity::Minor,
            overall: "1アプリ更新: ✨1".to_string(),
            packages: vec![PackageUpdate {
                name: name.to_string(),
                old: Some("1.0".to_string()),
                new: Some("1.1".to_string()),
                change: ChangeKind::Upgraded,
                declared: true,
                source: PackageSource::Nix,
                notes_url: None,
                change_items: Vec::new(),
            }],
        }
    }

    fn temp_path(suffix: &str) -> std::path::PathBuf {
        let mut dir = std::env::temp_dir();
        let unique = format!(
            "dotfiles-update-history-{}-{}.toml",
            std::process::id(),
            suffix
        );
        dir.push(unique);
        dir
    }

    #[test]
    fn read_missing_file_is_empty() -> crate::Result<()> {
        let path = temp_path("missing");
        let _ = std::fs::remove_file(&path);
        let adapter = TomlHistoryStoreAdapter::new(&path);
        assert!(adapter.read_entries()?.is_empty());
        Ok(())
    }

    #[test]
    fn read_directory_skips_registry_and_non_monthly_toml() -> crate::Result<()> {
        // 退行固定（finding 3369076725）: 履歴 directory に置かれる registry `notes-sources.toml` や
        // 月次形でない他 TOML は履歴連結読みから除外し、`<YYYY-MM>.toml` だけを読む。registry が
        // `[update]` テーブルを持つと旧実装は `update = Vec<UpdateEntry>` として誤読し show が壊れたため、
        // ファイル名規則で構造的に除外することを固定する。
        use super::is_monthly_history_file;

        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "dotfiles-history-dir-{}-skip-registry",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;

        // 正当な月次履歴ファイル（読まれる）。
        let monthly = dir.join("2026-06.toml");
        TomlHistoryStoreAdapter::new(&monthly)
            .append_entry(&sample("2026-06-01T00:00:00Z", "a"))?;

        // registry ファイル: パッケージ名 `update` を学習して `[update]` テーブルを持つケースを模す。
        // 拡張子だけで拾うと `update = Vec<UpdateEntry>` として誤読され read が落ちる。
        std::fs::write(dir.join("notes-sources.toml"), "[update]\nrepo = \"o/r\"\n")?;
        // 月次形でない他 TOML も除外されることを確認する。
        std::fs::write(dir.join("scratch.toml"), "key = \"value\"\n")?;

        // ファイル名規則の純粋判定。
        assert!(is_monthly_history_file("2026-06.toml"));
        assert!(!is_monthly_history_file("notes-sources.toml"));
        assert!(!is_monthly_history_file("scratch.toml"));
        assert!(!is_monthly_history_file("2026-6.toml"));
        assert!(!is_monthly_history_file("history.toml"));

        // directory 読みは registry/非月次を無視し、月次の 1 件だけを返す（read が落ちない）。
        let adapter = TomlHistoryStoreAdapter::new(&dir);
        let entries = adapter.read_entries()?;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].packages[0].name, "a");

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn append_preserves_existing_and_accumulates() -> crate::Result<()> {
        let path = temp_path("append");
        let _ = std::fs::remove_file(&path);
        let adapter = TomlHistoryStoreAdapter::new(&path);

        adapter.append_entry(&sample("2026-06-01T00:00:00Z", "a"))?;
        adapter.append_entry(&sample("2026-06-02T00:00:00Z", "b"))?;

        let entries = adapter.read_entries()?;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].at, "2026-06-01T00:00:00Z");
        assert_eq!(entries[1].packages[0].name, "b");

        let _ = std::fs::remove_file(&path);
        Ok(())
    }
}
