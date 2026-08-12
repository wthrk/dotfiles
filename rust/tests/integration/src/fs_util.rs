//! host 側 integration runner が使う filesystem helper。

use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    os::unix::ffi::OsStrExt,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, anyhow, bail};

use crate::Result;

/// repository が無視しないファイルだけを runtime source tree として複製する。
pub(crate) fn copy_repo_source(source: &Path, destination: &Path) -> Result<()> {
    if destination == source || destination.starts_with(source) {
        return Err(anyhow!(
            "runtime source copy の出力先は repository 外である必要があります: {}",
            destination.display()
        ));
    }
    let ignore = IgnoredPaths::read(source)?;
    copy_dir_filtered(source, source, destination, &ignore)
}

fn copy_dir_filtered(
    root: &Path,
    source: &Path,
    destination: &Path,
    ignore: &IgnoredPaths,
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

/// repository root からの相対パスで表した、複製対象から外す entry の集合。
///
/// 判定は git 自身に委ねる。`.gitignore` の否定パターン、入れ子の `.gitignore`、
/// `.git/info/exclude` を repository 側で再実装すると、実際の追跡対象と食い違ったまま
/// 複製が進む。`--directory` を渡すので、丸ごと無視される directory は 1 entry として返り、
/// その配下を歩く前に打ち切れる。
struct IgnoredPaths {
    paths: HashSet<PathBuf>,
}

impl IgnoredPaths {
    fn read(root: &Path) -> Result<Self> {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "ls-files",
                "-z",
                "--others",
                "--ignored",
                "--exclude-standard",
                "--directory",
            ])
            .output()
            .with_context(|| format!("git ls-files を起動できません: {}", root.display()))?;
        if !output.status.success() {
            bail!(
                "git ls-files が失敗しました: {}: {}",
                root.display(),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(Self {
            paths: output
                .stdout
                .split(|byte| *byte == 0)
                .filter(|entry| !entry.is_empty())
                .map(|entry| {
                    // directory entry は末尾の `/` 付きで返るため、walk 側の相対パス表現へ揃える。
                    let entry = entry.strip_suffix(b"/").unwrap_or(entry);
                    PathBuf::from(OsStr::from_bytes(entry))
                })
                .collect(),
        })
    }

    fn matches(&self, path: &Path) -> bool {
        self.paths.contains(path)
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
