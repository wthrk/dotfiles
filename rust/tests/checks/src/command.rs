//! 静的検証と zsh 検証で共通に使うログとユーザー名取得。

use std::process::Command;

use crate::Result;
use anyhow::bail;

/// 長い検証ログで失敗位置を追えるよう、各検証ブロックの開始を同じ形式で出力する。
pub fn step(label: &str) {
    println!("==> {label}");
}

/// zsh 検証用の Home Manager 設定名に使うログイン名を `id -un` から読む。
pub fn current_user() -> Result<String> {
    let output = Command::new("id").arg("-un").output()?;
    if !output.status.success() {
        bail!("id -un command failed");
    }
    let user = String::from_utf8(output.stdout)?;
    let user = user.trim();
    if user.is_empty() {
        bail!("user is empty");
    }
    Ok(user.to_string())
}
