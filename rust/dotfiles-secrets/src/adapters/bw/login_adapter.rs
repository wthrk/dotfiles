//! `BwLoginPort` を Bitwarden Password Manager CLI（`bw`）の login / unlock 子プロセスへ接続する adapter。
//!
//! `bw` CLI は spec L84 / L192 で唯一許可された外部 CLI 例外であり、用途は login / unlock に限る。この adapter
//! だけが `bw login` / `bw unlock` の子プロセスを起動する（`Command::new` はこの境界に閉じる）。master password
//! （`bw-password`）は protection 境界の borrow からそのまま子プロセスの `BW_PASSWORD` env へ注入し、argv / ログ
//! / shell history / 一時ファイル / 親プロセスの永続環境変数へは残さない。login email / OTP は domain で検証済みの
//! 非秘匿 argv 値として渡し、session key（`bw unlock --raw` の stdout）だけを domain 値として返す。
//!
//! application は YubiKey 由来 secret の取得順序と email override 判断を持つ（use case 順序の所有）。この adapter
//! は順序判断や secret 取得を行わず、与えられた検証済み入力を `bw` CLI 呼び出しへ翻訳するだけにする。

use std::process::{Command, Stdio};

use crate::Result;
use crate::{
    domain::bw_login::{BW_OTP_TWO_FACTOR_METHOD, BwLoginEmail, BwOtp, BwSessionKey},
    ports::bw::BwLoginPort,
    support::protection::{ProtectedSecret, bw_login},
};

/// 子プロセスの env で master password を受け取らせる env 変数名。`bw --passwordenv` の対象。
const BW_PASSWORD_ENV: &str = "BW_PASSWORD";
/// 起動する Bitwarden Password Manager CLI の program 名。
const BW_PROGRAM: &str = "bw";

impl BwLoginPort for super::BwLoginAdapter {
    async fn login_and_unlock(
        &self,
        email: &BwLoginEmail,
        password: &ProtectedSecret,
        otp: &BwOtp,
    ) -> Result<BwSessionKey> {
        // master password 平文を protection の borrow 境界の内側だけで取り出し、その borrow 中に
        // `bw login` / `bw unlock` を実行する。平文は closure 外へ持ち出さず、`BW_PASSWORD` env でだけ
        // 子プロセスへ渡す。Command の組み立てと実行（`Command::new`）はこの closure に閉じる。
        bw_login::with_master_password(password, |password_plaintext| {
            run_login(email, password_plaintext, otp)?;
            run_unlock(password_plaintext)
        })
    }
}

/// `bw login <email> --passwordenv BW_PASSWORD --method 3 --code <otp>` を実行する。
///
/// master password は argv へ載せず `BW_PASSWORD` env でだけ渡す。login の stdout/stderr は session key を
/// 含まないため継承せず破棄し、失敗時は終了コードだけで停止する（secret を診断へ混ぜない）。
fn run_login(email: &BwLoginEmail, password_plaintext: &str, otp: &BwOtp) -> Result<()> {
    let status = base_command(password_plaintext)
        .arg("login")
        .arg(email.as_str())
        .arg("--passwordenv")
        .arg(BW_PASSWORD_ENV)
        .arg("--method")
        .arg(BW_OTP_TWO_FACTOR_METHOD)
        .arg("--code")
        .arg(otp.as_str())
        // login の出力は session key を含まないため継承せず破棄する。
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| anyhow::anyhow!("failed to run `bw login`: {error}"))?;
    if !status.success() {
        anyhow::bail!("`bw login` failed; check the YubiKey OTP and Bitwarden credentials");
    }
    Ok(())
}

/// `bw unlock --passwordenv BW_PASSWORD --raw` を実行し、stdout の session key を返す。
///
/// `--raw` は session key だけを stdout に出すため、stdout を capture して domain 検証する。master password は
/// argv へ載せず `BW_PASSWORD` env でだけ渡す。失敗時は session key を返さず停止する。
fn run_unlock(password_plaintext: &str) -> Result<BwSessionKey> {
    let output = base_command(password_plaintext)
        .arg("unlock")
        .arg("--passwordenv")
        .arg(BW_PASSWORD_ENV)
        .arg("--raw")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| anyhow::anyhow!("failed to run `bw unlock`: {error}"))?;
    if !output.status.success() {
        anyhow::bail!("`bw unlock` failed; the Bitwarden master password may be incorrect");
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|_| anyhow::anyhow!("`bw unlock` returned a non-UTF-8 session key"))?;
    BwSessionKey::parse(&stdout)
}

/// `BW_PASSWORD` env を設定した `bw` 子プロセスの基礎 Command を作る。
///
/// master password は子プロセスの env だけに載せ、親プロセスの永続環境変数へは設定しない（`Command::env` は
/// 子プロセス限定）。継承した親 env から `BW_PASSWORD` が漏れることがないよう、子プロセスの env を明示設定して
/// 上書きする。argv には master password を一切載せない。
fn base_command(password_plaintext: &str) -> Command {
    let mut command = Command::new(BW_PROGRAM);
    command.env(BW_PASSWORD_ENV, password_plaintext);
    command
}
