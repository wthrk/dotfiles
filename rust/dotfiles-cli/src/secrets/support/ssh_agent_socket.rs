//! gpg-agent の SSH agent socket と GnuPG home を解決する process-generic な技術 primitive。
//!
//! この module は use case 名や device 選択方針を知らず、環境変数（`GNUPGHOME` / `HOME` /
//! `SSH_AUTH_SOCK`）と filesystem の socket 判定だけを扱う。設計「zsh 環境変数決定」と
//! `config/zsh/env.zsh` の上書き条件に合わせ、`${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` が socket なら
//! それを優先し、socket でない場合だけ既存環境変数 `SSH_AUTH_SOCK` が指す socket へ fallback する。
//! `gpgconf` CLI は使わない。#14 の `ssh_agent_adapter` と #15 の `clone_adapter` がこの解決規則を共有する。

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::Result;

/// `${GNUPGHOME:-$HOME/.gnupg}` を解決する。
///
/// `GNUPGHOME` が設定されていればそれを、未設定なら `$HOME/.gnupg` を返す。`HOME` も無い場合は失敗する。
pub(crate) fn gnupg_home() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("GNUPGHOME") {
        return Ok(PathBuf::from(home));
    }
    let home = std::env::var_os("HOME").context("HOME is not set; cannot resolve GnuPG home")?;
    Ok(PathBuf::from(home).join(".gnupg"))
}

/// SSH agent socket を解決する。
///
/// `${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` が socket ならそれを優先し、socket でない場合だけ既存
/// 環境変数 `SSH_AUTH_SOCK` が指す path が socket ならそれへ fallback する。どちらも socket でなければ
/// `None` を返し、呼び出し側は socket 未解決として停止条件へ反映する。`gpgconf` CLI は使わない。
pub(crate) fn resolve_ssh_agent_socket() -> Option<PathBuf> {
    if let Ok(home) = gnupg_home() {
        let fixed = home.join("S.gpg-agent.ssh");
        if is_socket(&fixed) {
            return Some(fixed);
        }
    }
    if let Some(env) = std::env::var_os("SSH_AUTH_SOCK") {
        let env = PathBuf::from(env);
        if is_socket(&env) {
            return Some(env);
        }
    }
    None
}

/// 指定 path が socket として存在するかを返す。
fn is_socket(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        std::fs::metadata(path)
            .map(|metadata| metadata.file_type().is_socket())
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}
