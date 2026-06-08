//! `NotesSourceRegistryPort` を `docs/update-history/notes-sources.toml` のファイル I/O へ接続する adapter。
//!
//! ノート取得元レジストリ（provenance の学習・再利用。利用者要件 (3)/(4)）を read/write する。read は
//! 不存在なら空レジストリを返し、write はレジストリ全体を決定論（パッケージ名昇順）で直列化して書き戻す。
//! 名前昇順は domain（[`NotesSourceRegistry`] の `BTreeMap`）が保証し、本 adapter はその直列化結果を
//! そのまま書くだけのため diff が最小化される。serde derive を介した TOML encode/decode の具体実装は本
//! adapter に閉じ、domain の wire 型は `toml` クレートへ依存しない。参照優先・自己修復・origin 別の再探索
//! 要否は application/domain の責務であり、本 adapter は単純な永続化境界（全体 read / 全体 write）に限定する。

use std::path::{Path, PathBuf};

use crate::Result;
use crate::update_history::domain::registry::NotesSourceRegistry;
use crate::update_history::ports::NotesSourceRegistryPort;

/// 単一レジストリファイルへの read/write を `NotesSourceRegistryPort` 契約へ翻訳する adapter。
pub(in crate::update_history) struct TomlNotesSourceRegistryAdapter {
    /// 対象 `docs/update-history/notes-sources.toml` の絶対 or 相対パス。
    path: PathBuf,
}

impl TomlNotesSourceRegistryAdapter {
    /// 対象レジストリファイルパスを束ねた adapter を作る。
    pub(in crate::update_history) fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// 既存ファイルを読み、レジストリを返す（不存在なら空レジストリ）。
    fn read_file(path: &Path) -> Result<NotesSourceRegistry> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(toml::from_str(&text)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(NotesSourceRegistry::default())
            }
            Err(error) => Err(error.into()),
        }
    }
}

impl NotesSourceRegistryPort for TomlNotesSourceRegistryAdapter {
    fn read_registry(&self) -> Result<NotesSourceRegistry> {
        Self::read_file(&self.path)
    }

    fn write_registry(&self, registry: &NotesSourceRegistry) -> Result<()> {
        // 書き込み先 directory（`docs/update-history`）が無い初回でも書けるよう親 directory を確保する。
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        // domain の BTreeMap が名前昇順を保証するため、直列化結果は決定論で diff が最小化される。
        let rendered = toml::to_string(registry)?;
        std::fs::write(&self.path, rendered)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! read（不存在で空）と write（決定論・往復保存）をテンポラリファイルで固定する。

    use super::TomlNotesSourceRegistryAdapter;
    use crate::update_history::domain::diff::DeltaSource;
    use crate::update_history::domain::registry::{
        NotesOrigin, NotesSourceEntry, NotesSourceRegistry,
    };
    use crate::update_history::ports::NotesSourceRegistryPort;

    fn temp_path(suffix: &str) -> std::path::PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "dotfiles-notes-sources-{}-{}.toml",
            std::process::id(),
            suffix
        ));
        dir
    }

    fn entry(source: Option<&str>, origin: NotesOrigin) -> NotesSourceEntry {
        NotesSourceEntry {
            source: source.map(str::to_string),
            origin,
            discovered_at: Some("2026-06-07T00:00:00Z".to_string()),
            note: None,
        }
    }

    #[test]
    fn read_missing_registry_is_empty() -> crate::Result<()> {
        let path = temp_path("missing");
        let _ = std::fs::remove_file(&path);
        let adapter = TomlNotesSourceRegistryAdapter::new(&path);
        assert_eq!(adapter.read_registry()?, NotesSourceRegistry::default());
        Ok(())
    }

    #[test]
    fn write_then_read_round_trips() -> crate::Result<()> {
        let path = temp_path("roundtrip");
        let _ = std::fs::remove_file(&path);
        let adapter = TomlNotesSourceRegistryAdapter::new(&path);

        let mut registry = NotesSourceRegistry::default();
        registry.record(
            "neovim",
            DeltaSource::NixEval,
            entry(
                Some("https://github.com/neovim/neovim/releases"),
                NotesOrigin::Mechanical,
            ),
        );
        registry.record("zlib", DeltaSource::NixEval, entry(None, NotesOrigin::None));

        adapter.write_registry(&registry)?;
        let read = adapter.read_registry()?;
        assert_eq!(read, registry);

        let _ = std::fs::remove_file(&path);
        Ok(())
    }
}
