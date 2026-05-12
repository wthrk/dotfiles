//! `dotfiles` CLI が省略値として使う環境情報を集める。
//!
//! ユーザー名、ホスト名、システム名、設定ディレクトリは `init` と `switch` の両方で使う。
//! 取得方法をここに閉じ込め、生成される flake の出力名と CLI が参照する出力名を一致させる。

use std::path::PathBuf;
use std::process::Command;

use crate::Result;
use anyhow::{anyhow, bail};
use dotfiles_core::host;

const CONFIG_SUBDIR: &str = ".config/dotfiles";
const CONFIG_FILE: &str = "flake.nix";

/// 明示された設定ディレクトリを優先し、省略時は `$HOME/.config/dotfiles` を返す。
pub(crate) fn config_dir(override_dir: Option<PathBuf>) -> Result<PathBuf> {
    Ok(override_dir.unwrap_or(home_dir()?.join(CONFIG_SUBDIR)))
}

/// `dotfiles init` が書き込む `flake.nix` のパスを返す。
pub(crate) fn config_path(override_dir: Option<PathBuf>) -> Result<PathBuf> {
    Ok(config_dir(override_dir)?.join(CONFIG_FILE))
}

/// `homeConfigurations.<user>` として使うログイン名を読む。
pub(crate) fn current_user() -> Result<String> {
    let output = Command::new("id").arg("-un").output()?;
    if !output.status.success() {
        bail!("id -un command failed");
    }
    let user = String::from_utf8(output.stdout)?;
    nonempty("user")(user.trim().to_string())
}

/// `darwinConfigurations.<host>` として使う短いホスト名を読む。
pub(crate) fn current_host() -> Result<String> {
    let output = Command::new("hostname").output()?;
    if !output.status.success() {
        bail!("hostname command failed");
    }
    let host = String::from_utf8(output.stdout)?;
    let host = host::short(host.trim());
    if host.is_empty() {
        bail!("host is required")
    } else {
        Ok(host.to_string())
    }
}

/// 現在の OS と CPU から、ローカル flake に記録する既定の Nix system 文字列を作る。
pub(crate) fn default_system() -> String {
    let arch = std::env::consts::ARCH;
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        other => other,
    };
    format!("{arch}-{os}")
}

/// 設定ファイルの配置先を決めるため `$HOME` を必須値として読む。
fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| anyhow!("HOME is required"))
}

/// 環境由来の値が空文字なら、後続の flake 出力名として使う前に失敗させる。
fn nonempty(name: &'static str) -> impl FnOnce(String) -> Result<String> {
    move |value| {
        if value.is_empty() {
            bail!("{name} is empty")
        } else {
            Ok(value)
        }
    }
}

#[cfg(test)]
mod tests {
    use dotfiles_core::host;

    #[test]
    fn uses_short_hostname() {
        // CLI と生成 flake の出力名は同じ短いホスト名を使う必要がある。
        // ここがずれると `dotfiles switch darwin` が別の属性を参照する。
        assert_eq!(host::short("macbook.local"), "macbook");
        assert_eq!(host::short("macbook"), "macbook");
    }
}
