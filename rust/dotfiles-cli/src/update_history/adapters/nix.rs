//! `NixVersionPort` を `nix eval` 由来の name→version JSON ファイルへ接続する adapter。
//!
//! eval ベース化により、nightly は ci-ref のフル closure を `nix store diff-closures` で 2 回ビルド
//! する代わりに、宣言パッケージの `pname`/`version`（評価時属性。ビルド/フェッチ不要）を `nix eval
//! --json` で数秒で取得する。CI は old lock（bump 前）と new lock（bump 後）でそれぞれ eval し、その
//! `{ "name": "version", ... }` JSON をファイルへ書く。本 adapter はその 2 ファイルを読んで `BTreeMap`
//! へ翻訳する境界であり、version 比較・差分種別の業務意味は domain rule（[`diff_versions`]）に委ねる。
//!
//! eval JSON ファイルが与えられない／読めない実行環境では、record を失敗させず空マップを返す
//! （「差分取得不能はフォールバックして version+notes_url へ縮退」のプラン契約に沿う graceful
//! degradation）。本 adapter は「JSON ファイル読み取りと name→version マップ翻訳」という外部 I/O
//! 翻訳だけを担い、eval プロセス実行自体は CI（信頼 ref）が行う。

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::Result;
use crate::update_history::domain::diff::NixPackage;
use crate::update_history::ports::NixVersionPort;

/// `nix eval` 由来の old/new name→version JSON ファイルを `NixVersionPort` 契約へ翻訳する adapter。
///
/// `old`/`new` は CI が ci-ref の bump 前/後 lock で eval した name→version JSON ファイルの path。
/// いずれも `None` または読めない場合はその側を空マップで返す（縮退）。
#[derive(Default)]
pub(in crate::update_history) struct NixEvalVersionAdapter {
    /// bump 前 lock の eval JSON ファイル path。未設定なら old は空マップ。
    old: Option<PathBuf>,
    /// bump 後 lock の eval JSON ファイル path。未設定なら new は空マップ。
    new: Option<PathBuf>,
}

impl NixEvalVersionAdapter {
    /// old/new eval JSON ファイル path を束ねた adapter を作る。`None` で当該側を縮退（空マップ）にする。
    pub(in crate::update_history) fn new(old: Option<PathBuf>, new: Option<PathBuf>) -> Self {
        Self { old, new }
    }

    /// eval JSON ファイルを読んで name→[`NixPackage`] マップへ翻訳する。
    ///
    /// path が `None` またはファイル不存在なら空マップを返す（縮退）。それ以外の I/O / JSON parse 失敗は
    /// `Err` で伝播し、部分的に壊れた差分を作らない。`nix eval --json --apply` の出力は
    /// `{ "name": { "version": "...", "repo": "owner/repo", "changelog": "..." }, ... }` object である
    /// ことを契約とする（`repo` は GitHub owner/repo、`changelog` は `meta.changelog`/`meta.homepage` 由来。
    /// eval 側で常に出力されるが、欠落時は `NixPackage` の `serde(default)` で空文字へ縮退し、旧 `notes_source`
    /// key も `changelog` の alias で受ける）。
    fn read_map(path: &Option<PathBuf>) -> Result<BTreeMap<String, NixPackage>> {
        let Some(path) = path else {
            return Ok(BTreeMap::new());
        };
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BTreeMap::new());
            }
            Err(error) => return Err(error.into()),
        };
        let map: BTreeMap<String, NixPackage> = serde_json::from_str(&text)?;
        Ok(map)
    }
}

impl NixVersionPort for NixEvalVersionAdapter {
    fn old_versions(&self) -> Result<BTreeMap<String, NixPackage>> {
        Self::read_map(&self.old)
    }

    fn new_versions(&self) -> Result<BTreeMap<String, NixPackage>> {
        Self::read_map(&self.new)
    }
}

#[cfg(test)]
mod tests {
    //! eval JSON ファイルの読み取りと name→version マップ翻訳（正常 / 不在縮退）を固定する。

    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::NixEvalVersionAdapter;
    use crate::Result;
    use crate::update_history::ports::NixVersionPort;

    /// テスト一時ファイルへ JSON を書き、その path を返す。書込み失敗は `Result` で伝播する
    /// （`unwrap`/`expect` 禁止）。
    fn write_temp(name: &str, content: &str) -> Result<PathBuf> {
        let mut path = std::env::temp_dir();
        path.push(format!("dotfiles-nix-eval-test-{name}.json"));
        std::fs::write(&path, content)?;
        Ok(path)
    }

    #[test]
    fn reads_name_version_repo_changelog_json() -> Result<()> {
        let old = write_temp(
            "old",
            r#"{"neovim":{"version":"0.10.2","repo":"neovim/neovim","changelog":"https://github.com/neovim/neovim/blob/master/CHANGELOG.md"},"zlib":{"version":"1.3.1","repo":"","changelog":""}}"#,
        )?;
        let new = write_temp(
            "new",
            r#"{"neovim":{"version":"0.11.0","repo":"neovim/neovim","changelog":"https://github.com/neovim/neovim/blob/master/CHANGELOG.md"},"zlib":{"version":"1.3.1","repo":"","changelog":""}}"#,
        )?;
        let adapter = NixEvalVersionAdapter::new(Some(old), Some(new));

        let old_map = adapter.old_versions()?;
        let new_map = adapter.new_versions()?;
        assert_eq!(
            old_map.get("neovim").map(|p| p.version.as_str()),
            Some("0.10.2")
        );
        assert_eq!(
            new_map.get("neovim").map(|p| p.version.as_str()),
            Some("0.11.0")
        );
        // repo（GitHub owner/repo）を読み取る。
        assert_eq!(
            new_map.get("neovim").map(|p| p.repo.as_str()),
            Some("neovim/neovim")
        );
        // changelog（meta.changelog/homepage 由来）も読み取る。
        assert_eq!(
            new_map.get("neovim").map(|p| p.notes_source.as_str()),
            Some("https://github.com/neovim/neovim/blob/master/CHANGELOG.md")
        );
        Ok(())
    }

    #[test]
    fn missing_repo_and_changelog_fields_default_to_empty() -> Result<()> {
        // repo/changelog 欠落（旧形式や github 由来不明）は serde(default) で空文字へ縮退する。
        let new = write_temp("new-no-notes", r#"{"git":{"version":"2.54.0"}}"#)?;
        let adapter = NixEvalVersionAdapter::new(None, Some(new));
        let new_map = adapter.new_versions()?;
        assert_eq!(
            new_map.get("git").map(|p| p.version.as_str()),
            Some("2.54.0")
        );
        assert_eq!(new_map.get("git").map(|p| p.repo.as_str()), Some(""));
        assert_eq!(
            new_map.get("git").map(|p| p.notes_source.as_str()),
            Some("")
        );
        Ok(())
    }

    #[test]
    fn reads_legacy_notes_source_key_via_alias() -> Result<()> {
        // 旧 JSON key `notes_source` も `changelog` の alias で受ける（後方互換）。
        let new = write_temp(
            "new-legacy",
            r#"{"git":{"version":"2.54.0","notes_source":"https://github.com/git/git"}}"#,
        )?;
        let adapter = NixEvalVersionAdapter::new(None, Some(new));
        let new_map = adapter.new_versions()?;
        assert_eq!(
            new_map.get("git").map(|p| p.notes_source.as_str()),
            Some("https://github.com/git/git")
        );
        Ok(())
    }

    #[test]
    fn missing_path_degrades_to_empty_map() -> Result<()> {
        let adapter = NixEvalVersionAdapter::new(None, None);
        assert_eq!(adapter.old_versions()?, BTreeMap::new());
        assert_eq!(adapter.new_versions()?, BTreeMap::new());
        Ok(())
    }

    #[test]
    fn nonexistent_file_degrades_to_empty_map() -> Result<()> {
        let mut missing = std::env::temp_dir();
        missing.push("dotfiles-nix-eval-test-does-not-exist.json");
        let _ = std::fs::remove_file(&missing);
        let adapter = NixEvalVersionAdapter::new(Some(missing), None);
        assert_eq!(adapter.old_versions()?, BTreeMap::new());
        Ok(())
    }
}
