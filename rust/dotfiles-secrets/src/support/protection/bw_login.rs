//! Bitwarden Password Manager CLI の master password を `BW_PASSWORD` env 注入境界の内側だけで扱う操作。
//!
//! `bw login` / `bw unlock` は master password を子プロセスの `BW_PASSWORD` env でだけ受け取る（spec L86 /
//! L178）。`bw-password` は [`ProtectedSecret`] であり、その平文は `with_secret` の借用境界の内側だけで公開
//! できる。この module は、adapter が組み立てた child runner closure に対して、借用中の平文 password だけを
//! 渡す broker を提供する。平文を closure 外へ持ち出さず、argv / ログ / shell history / 永続環境変数 /
//! 一時ファイルへ残さない責務はこの境界で担う。Command の組み立てと実行（`Command::new`）は adapter 側の
//! closure にあり、この module は process 実行詳細を持たない。
#![cfg_attr(feature = "secrets-internal-test-stub", allow(dead_code))]

use crate::Result;
use crate::domain::bw_login::BwLoginEmail;
use crate::support::protection::ProtectedSecret;

/// YubiKey から読み出した `bw-email` 保護値を borrow 境界の内側で argv 安全な login email へ翻訳する。
///
/// `bw-email` は credential ではないが YubiKey storage 上では他 secret と同じ [`ProtectedSecret`] で運ばれる。
/// argv に載せるために平文へ変換する必要があり、その変換は `with_secret` を呼べる protection 境界の内側で
/// 行う。検証（空文字・制御文字の排除）は domain rule [`BwLoginEmail::parse`] に委ね、検証済みの 1 行だけを
/// 返す。email が UTF-8 でない場合は失敗する。
pub(crate) fn parse_email(email: &ProtectedSecret) -> Result<BwLoginEmail> {
    email.with_secret_password_str(BwLoginEmail::parse)
}

/// master password の平文を借用境界の内側だけで `runner` へ渡す。
///
/// `runner` は借用中の `&str` password を `BW_PASSWORD` の値として子プロセスへ注入し、`bw` CLI を実行して
/// 結果を返す closure である。password 平文はこの関数の borrow scope の内側だけに存在し、closure から
/// 返値として持ち出してはならない（呼び出し側 adapter の責務）。password が UTF-8 でない場合は失敗する。
pub(crate) fn with_master_password<R>(
    password: &ProtectedSecret,
    runner: impl FnOnce(&str) -> Result<R>,
) -> Result<R> {
    password.with_secret_password_str(runner)
}
