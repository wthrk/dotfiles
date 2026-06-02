//! Bitwarden Password Manager CLI（`bw`）login / unlock の secret 保護境界 backend 操作。
//!
//! `bw login` / `bw unlock` は master password を要求する外部 command である。secret-handling.md の
//! 外部処理境界に従い、master password の借用とその子プロセスへの受け渡しはこの protection 境界内で
//! 完了させる。master password は `BW_PASSWORD` env として子プロセスにだけ渡し、argv・ログ・親プロセスの
//! 永続環境変数・一時ファイルへ残さない。`BW_SESSION` 値そのものは呼び出し側へ返さない。
//!
//! この module は `gpg-backend`（= 非 `secrets-internal-test-stub`）build でだけ compile し、internal
//! test stub build では adapter 側 stub と compile-time で差し替わる。

use std::process::Command;

use anyhow::{Context, bail};

use crate::Result;
use crate::secrets::support::protection::ProtectedSecret;

/// `bw login <email> --passwordenv BW_PASSWORD --method 3 --code <otp>` の後
/// `bw unlock --passwordenv BW_PASSWORD --raw` を実行する（spec L178）。
///
/// master password は `password.with_secret_utf8` の借用境界内でだけ `BW_PASSWORD` env value へ複製し、`bw`
/// の子プロセスへ env として渡す。借用 closure を抜けると複製 buffer は zeroize 管理（`Zeroizing`）で破棄
/// される。email は非秘匿だが、YubiKey 由来 email と同じ carrier 型で受け取るため `ProtectedSecret` で渡され、
/// この借用境界内で `&str` へ取り出して `bw login` の位置引数に使う。otp は非秘匿のワンタイムコードで `--code`
/// の値として渡す。login または unlock が非ゼロ終了した場合は停止条件として `Err` を返す。`bw unlock --raw` の
/// stdout は `BW_SESSION` 値（secret）であり、ここでは成立確認にだけ使い呼び出し側へ返さない。
pub(crate) fn login_and_unlock(
    email: &ProtectedSecret,
    password: &ProtectedSecret,
    otp: &str,
) -> Result<()> {
    email.with_secret_utf8(|email| run_bw_login(email, password, otp))?;
    run_bw_unlock(password)?;
    Ok(())
}

/// `bw` CLI への到達可能性だけを確認する（spec L155 / L201 の `--check bw-login`）。
///
/// login / unlock を成立させず、`bw --version` の成功で `bw` CLI の利用可否だけを確認する。secret は要求しない。
pub(crate) fn check_reachable() -> Result<()> {
    let status = Command::new("bw")
        .arg("--version")
        .status()
        .context("failed to invoke `bw` CLI for bw-login reachability check")?;
    if !status.success() {
        bail!("`bw` CLI is not available for bw-login reachability check");
    }
    Ok(())
}

/// `bw login` を master password env つきで実行する。
fn run_bw_login(email: &str, password: &ProtectedSecret, otp: &str) -> Result<()> {
    let success = password.with_secret_utf8(|password| {
        let status = Command::new("bw")
            .arg("login")
            .arg(email)
            .arg("--passwordenv")
            .arg("BW_PASSWORD")
            .arg("--method")
            .arg("3")
            .arg("--code")
            .arg(otp)
            .env("BW_PASSWORD", password)
            .status()
            .context("failed to invoke `bw login`")?;
        Ok(status.success())
    })?;
    if !success {
        bail!("`bw login` failed");
    }
    Ok(())
}

/// `bw unlock --raw` を master password env つきで実行する。stdout の `BW_SESSION` は呼び出し側へ返さない。
fn run_bw_unlock(password: &ProtectedSecret) -> Result<()> {
    let success = password.with_secret_utf8(|password| {
        let status = Command::new("bw")
            .arg("unlock")
            .arg("--passwordenv")
            .arg("BW_PASSWORD")
            .arg("--raw")
            .env("BW_PASSWORD", password)
            .status()
            .context("failed to invoke `bw unlock`")?;
        Ok(status.success())
    })?;
    if !success {
        bail!("`bw unlock` failed");
    }
    Ok(())
}
