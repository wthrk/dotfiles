//! host 側 integration runner が使う filesystem helper。

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use anyhow::{Context, anyhow};

use crate::Result;

/// repository の `.gitignore` に従って runtime source tree を作る。
pub(crate) fn copy_repo_source(source: &Path, destination: &Path) -> Result<()> {
    if destination == source || destination.starts_with(source) {
        return Err(anyhow!(
            "runtime source copy の出力先は repository 外である必要があります: {}",
            destination.display()
        ));
    }
    let ignore = GitIgnore::read(source)?;
    copy_dir_filtered(source, source, destination, &ignore)
}

fn copy_dir_filtered(
    root: &Path,
    source: &Path,
    destination: &Path,
    ignore: &GitIgnore,
) -> Result<()> {
    if destination.exists() {
        fs::remove_dir_all(destination).with_context(|| {
            format!(
                "copy 先 directory の削除に失敗しました: {}",
                destination.display()
            )
        })?;
    }
    fs::create_dir_all(destination).with_context(|| {
        format!(
            "copy 先 directory の作成に失敗しました: {}",
            destination.display()
        )
    })?;

    for entry in fs::read_dir(source)
        .with_context(|| format!("copy 元 directory を読めません: {}", source.display()))?
    {
        let entry = entry.with_context(|| {
            format!("copy 元 directory entry を読めません: {}", source.display())
        })?;
        let source_path = entry.path();
        let relative_path = source_path.strip_prefix(root).with_context(|| {
            format!(
                "{} is not under repository root {}",
                source_path.display(),
                root.display()
            )
        })?;
        if relative_path == Path::new(".git") || ignore.matches(relative_path) {
            continue;
        }

        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .with_context(|| format!("file type を読めません: {}", source_path.display()))?;
        if file_type.is_dir() {
            copy_dir_filtered(root, &source_path, &destination_path, ignore)?;
        } else if file_type.is_symlink() {
            let link_target = fs::read_link(&source_path).with_context(|| {
                format!("symlink target を読めません: {}", source_path.display())
            })?;
            std::os::unix::fs::symlink(link_target, &destination_path).with_context(|| {
                format!(
                    "symlink の作成に失敗しました: {}",
                    destination_path.display()
                )
            })?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "file copy に失敗しました: {} -> {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
            let permissions = entry
                .metadata()
                .with_context(|| format!("metadata を読めません: {}", source_path.display()))?
                .permissions();
            fs::set_permissions(&destination_path, permissions).with_context(|| {
                format!(
                    "permissions の反映に失敗しました: {}",
                    destination_path.display()
                )
            })?;
        }
    }

    Ok(())
}

struct GitIgnore {
    patterns: Vec<IgnorePattern>,
}

impl GitIgnore {
    fn read(root: &Path) -> Result<Self> {
        let gitignore = root.join(".gitignore");
        let text = fs::read_to_string(&gitignore)
            .with_context(|| format!("{} を読めません", gitignore.display()))?;
        let patterns = text
            .lines()
            .filter_map(IgnorePattern::parse)
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { patterns })
    }

    fn matches(&self, path: &Path) -> bool {
        let path = path.to_string_lossy().replace('\\', "/");
        self.patterns.iter().any(|pattern| pattern.matches(&path))
    }
}

struct IgnorePattern {
    value: String,
    anchored: bool,
    directory: bool,
}

impl IgnorePattern {
    fn parse(line: &str) -> Option<Result<Self>> {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        if line.starts_with('!') {
            return Some(Err(anyhow!(
                "negated .gitignore patterns are not supported"
            )));
        }
        let directory = line.ends_with('/');
        let anchored = line.starts_with('/');
        let value = line.trim_start_matches('/').trim_end_matches('/');
        Some(Ok(Self {
            value: value.to_string(),
            anchored,
            directory,
        }))
    }

    fn matches(&self, path: &str) -> bool {
        if self.anchored {
            return path == self.value
                || path
                    .strip_prefix(&self.value)
                    .is_some_and(|rest| rest.starts_with('/'));
        }
        if self.directory {
            path.split('/').any(|component| component == self.value)
        } else {
            path == self.value || path.ends_with(&format!("/{}", self.value))
        }
    }
}

/// `CARGO_TARGET_DIR` が relative の場合は repository root 基準に解決する。
pub(crate) fn target_dir(repo_dir: &Path, cargo_target_dir: Option<&Path>) -> PathBuf {
    cargo_target_dir
        .map(|path| {
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                repo_dir.join(path)
            }
        })
        .unwrap_or_else(|| repo_dir.join("target"))
}

/// guest へ渡す binary に execute bit を付ける。
pub(crate) fn executable(path: &Path) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .with_context(|| format!("execute bit の設定に失敗しました: {}", path.display()))?;
    Ok(())
}
