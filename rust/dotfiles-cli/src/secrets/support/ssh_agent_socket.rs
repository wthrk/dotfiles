//! gpg-agent の SSH agent socket と GnuPG home を解決する process-generic な技術 primitive。
//!
//! この module は use case 名や device 選択方針を知らず、環境変数（`GNUPGHOME` / `HOME` /
//! `SSH_AUTH_SOCK`）と filesystem の socket 判定だけを扱う。設計「zsh 環境変数決定」と
//! `config/zsh/env.zsh` の上書き条件に合わせ、`${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` を
//! gpg-agent SSH support の socket として解決する。`gpgconf` CLI は使わない。
//!
//! 解決規則は用途で 2 種類に分ける。両者は gpg-agent socket の解決手順を共有し、fallback 可否だけ異なる。
//! - [`resolve_gpg_agent_socket`]: gpg-agent socket（`S.gpg-agent.ssh`）が socket のときだけ返す strict 解決。
//!   提示鍵を選べない経路（#15 の clone）で、通常の `ssh-agent` を指しうる `SSH_AUTH_SOCK` へ fallback しない。
//! - [`resolve_ssh_agent_socket`]: 上記を優先しつつ、socket でない場合だけ既存 `SSH_AUTH_SOCK` が指す socket へ
//!   fallback する。#14 の readiness 観測は対象 authentication subkey の key blob 一致で identity を照合する
//!   ため、fallback socket でも任意鍵を受け入れない。clone のように任意鍵を提示しうる経路はこちらを使わない。

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

/// gpg-agent の SSH support socket を strict に解決する（fallback なし）。
///
/// `${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` が socket ならそれを返し、socket でなければ `None` を返す。
/// 通常の `ssh-agent` を指しうる `SSH_AUTH_SOCK` へは fallback しない。提示する SSH identity を選べない経路
/// （#15 の password-store clone）は、GPG authentication subkey 由来 identity だけを使うためこの strict 解決を
/// 用い、任意鍵を提示しうる `ssh-agent` への fallback を作らない。`gpgconf` CLI は使わない。
pub(crate) fn resolve_gpg_agent_socket() -> Option<PathBuf> {
    let fixed = gnupg_home().ok()?.join("S.gpg-agent.ssh");
    is_socket(&fixed).then_some(fixed)
}

/// SSH agent socket を fallback 付きで解決する。
///
/// [`resolve_gpg_agent_socket`] の gpg-agent socket を優先し、それが socket でない場合だけ既存環境変数
/// `SSH_AUTH_SOCK` が指す path が socket ならそれへ fallback する。どちらも socket でなければ `None` を返し、
/// 呼び出し側は socket 未解決として停止条件へ反映する。#14 の readiness 観測は key blob 一致で identity を
/// 照合するため fallback socket でも安全だが、提示鍵を選べない経路はこの fallback 付き解決を使わない。
pub(crate) fn resolve_ssh_agent_socket() -> Option<PathBuf> {
    if let Some(gpg_agent) = resolve_gpg_agent_socket() {
        return Some(gpg_agent);
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
