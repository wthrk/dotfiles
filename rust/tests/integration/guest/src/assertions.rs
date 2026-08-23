//! ゲスト内で観測できるファイル状態を検査する処理。
//!
//! Nix の内部値を再検証するのではなく、switch 後に利用者が実際に受け取るリンクやファイルだけを見る。

use std::fs;
use std::path::Path;

use crate::Result;
use anyhow::bail;

/// 指定したホーム配下のファイルが Home Manager 由来の `/nix/store` リンクになっていることを検証する。
pub(crate) fn assert_managed_links(home: &str, paths: &[&str]) -> Result<()> {
    for relative in paths {
        let path = Path::new(home).join(relative);
        let meta = fs::symlink_metadata(&path)?;
        // 管理対象のホームファイルはシンボリックリンクである必要がある。通常ファイルの場合、
        // アクティベーションが期待する Home Manager 出力を導入できていない。
        if !meta.file_type().is_symlink() {
            bail!("{} is not a symlink", path.display());
        }
        let target = fs::read_link(&path)?;
        // リンク先は Nix store 由来である必要がある。
        // これにより、ローカルコピーの残骸ではなく Nix 管理ファイルだと確認する。
        if !target.starts_with("/nix/store") {
            bail!(
                "{} does not point into /nix/store: {}",
                path.display(),
                target.display()
            );
        }
    }
    Ok(())
}

/// nix-darwin が管理するユーザーが、期待した集合とちょうど一致することを検証する。
///
/// `/etc/profiles/per-user/` は `home-manager.useUserPackages` が system 層の管理対象ユーザーに
/// ついて作るディレクトリで、`dotfiles` CLI が scope 判定に使う唯一の材料でもある。ここに 2 人目の
/// エントリが現れることは、そのユーザーの flake が system 層を置き換えたことを意味する。
pub(crate) fn assert_system_profile_users(expected: &[&str]) -> Result<()> {
    let mut found = fs::read_dir("/etc/profiles/per-user")?
        .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
        .collect::<Result<Vec<_>>>()?;
    found.sort();
    let mut expected = expected
        .iter()
        .map(|user| (*user).to_string())
        .collect::<Vec<_>>();
    expected.sort();
    if found == expected {
        Ok(())
    } else {
        bail!("/etc/profiles/per-user has {found:?}, expected {expected:?}")
    }
}

/// シナリオの前提または成果物として必要なパスが存在することを検証する。
pub(crate) fn ensure_exists(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    if path.exists() {
        Ok(())
    } else {
        bail!("{} is missing", path.display())
    }
}

/// dry-run や no-switch が作ってはいけないパスについて、壊れたリンクも含めて不在を確認する。
pub(crate) fn ensure_absent_path(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    if path.exists() || fs::symlink_metadata(path).is_ok() {
        bail!("{} exists", path.display())
    } else {
        Ok(())
    }
}

/// 2 つのファイルが同じ内容かを返す。
pub(crate) fn files_have_same_content(
    left: impl AsRef<Path>,
    right: impl AsRef<Path>,
) -> Result<bool> {
    Ok(fs::read(left)? == fs::read(right)?)
}

/// ファイルが Nix store のパスを本文に含むかを返す。
///
/// 世代ごとに変わる値は store path として現れるため、世代に依らない内容かどうかの判定に使う。
pub(crate) fn file_mentions_store_path(path: impl AsRef<Path>) -> Result<bool> {
    Ok(String::from_utf8_lossy(&fs::read(path)?).contains("/nix/store/"))
}

/// symlink が Nix store の中を指しているかを返す。
pub(crate) fn link_points_into_store(path: impl AsRef<Path>) -> Result<bool> {
    Ok(fs::read_link(path)?.starts_with("/nix/store"))
}

/// flake や lock file など、空ファイルでは意味がない成果物の存在を確認する。
pub(crate) fn ensure_nonempty_path(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    if path.metadata()?.len() > 0 {
        Ok(())
    } else {
        bail!("{} is empty", path.display())
    }
}
