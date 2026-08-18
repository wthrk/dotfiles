//! `dotfiles update` が適用の直後に世代の掃除を起動することを、実行結果で検証する。
//!
//! 掃除は適用した層ごとに別のコマンドで、必要な権限も違う。`--dry-run` は外部コマンドを実行せずに
//! 起動列だけを標準出力へ書くので、その並びで「適用してから掃除する」順序を固定する。

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// 適用対象として渡す設定ディレクトリを用意する。
///
/// `--dry-run` は外部コマンドを起動しないため `flake.nix` の中身は評価されない。`dotfiles` が適用前に
/// 存在だけを確かめるので、空ファイルとして置く。
fn config_dir() -> anyhow::Result<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("update-collects-garbage");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("flake.nix"), "")?;
    Ok(dir)
}

/// 起動列の中で `before` を含む行より後に `after` を含む行が出るか。どちらかが無ければ偽。
fn runs_after(launched: &str, before: &str, after: &str) -> bool {
    let lines = launched.lines().collect::<Vec<_>>();
    let position = |needle: &str| lines.iter().position(|line| line.contains(needle));
    match (position(before), position(after)) {
        (Some(before), Some(after)) => before < after,
        _ => false,
    }
}

/// `update` は適用した層の世代の掃除を、その層の適用より後に起動する。
///
/// home 層は対象ユーザーの `home-manager expire-generations`、system 層は root の
/// `nix-collect-garbage` が対象になる。
#[test]
fn update_collects_garbage_after_switch() -> anyhow::Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_dotfiles"))
        .args([
            "update",
            "--dry-run",
            "--host",
            "tester-host",
            "--config-dir",
        ])
        .arg(config_dir()?)
        .output()?;

    let launched = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        runs_after(&launched, "home-manager switch", "expire-generations"),
        "{launched}{stderr}"
    );
    assert!(
        runs_after(&launched, "darwin-rebuild switch", "nix-collect-garbage"),
        "{launched}{stderr}"
    );
    Ok(())
}
