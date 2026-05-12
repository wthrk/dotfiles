//! 実行時コードとテストで同じ判定にしたいパス処理。

use std::env;
use std::path::{Path, PathBuf};

/// `Path` を外部コマンド引数に渡す直前の文字列表現へ変換する。
pub fn display(path: impl AsRef<Path>) -> String {
    path.as_ref().display().to_string()
}

/// ツールの存在確認用に `PATH` をたどる。`/` を含む場合は PATH 探索せず、そのパスだけを検証する。
pub fn find_executable(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let path = PathBuf::from(name);
        return is_executable_file(&path).then_some(path);
    }

    let path_var = env::var_os("PATH")?;
    env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|path| is_executable_file(path))
}

#[cfg(unix)]
/// Unix では通常ファイルかついずれかの実行ビットが立っていることを要求する。
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
/// 非 Unix では実行ビットを読めないため、通常ファイルの存在だけを要求する。
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}
