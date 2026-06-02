//! Bitwarden Password Manager CLI（`bw`）への login / unlock を application が要求する port 契約。
//!
//! この module は `bw login` / `bw unlock` という外部 command 境界の capability だけを宣言する。
//! `bw` CLI 引数の組み立て、`BW_PASSWORD` env による子プロセスへの secret 受け渡し、`BW_SESSION` の
//! 取り回し、process 実行の詳細は adapter 側（および secret 保護境界 backend）へ閉じ、ここには露出しない。
//! email / OTP は秘密情報ではないが、master password は `ProtectedSecret` を carrier として受け渡し、
//! 平文取り出し API は port へ持ち込まない。

use super::super::{domain::bw_login::BwLoginSummary, support::protection::ProtectedSecret};
use crate::Result;

/// use case が Bitwarden Password Manager CLI 境界へ要求する login / unlock 契約。
///
/// caller（application）は YubiKey 由来 secret の取得順序と OTP 入力順序を決め、検証済み入力を渡す。
/// implementor は `bw login <email> --passwordenv BW_PASSWORD --method 3 --code <otp>` の後
/// `bw unlock --passwordenv BW_PASSWORD --raw` を実行し（spec L178）、master password は `BW_PASSWORD`
/// env として子プロセスにだけ渡して保存しない。`BW_PASSWORD` を argv へ載せない。login / unlock の成立
/// だけを domain summary として返し、`BW_SESSION` 値そのもの（secret）を application へ返さない。
#[cfg_attr(test, mockall::automock)]
pub trait BwLoginPort {
    /// `bw login` の後 `bw unlock` を実行し、unlock 済み session の成立を summary として返す。
    ///
    /// `email` は非秘匿だが YubiKey 由来 email と override email を同じ carrier 型で受け取るため
    /// `ProtectedSecret` で渡す。`otp` は非秘匿のワンタイムコード、`password` は `BW_PASSWORD` env でのみ
    /// 子プロセスへ渡す保護値である。implementor は master password を argv / ログ / 永続環境変数 / 一時
    /// ファイルへ残さず、`BW_SESSION` は本 command 仕様に従って扱う。email / password の借用は protection
    /// 境界内で完了させる。login / unlock のいずれかが失敗した場合は停止条件として `Err` を返す。
    fn login_and_unlock(
        &self,
        email: &ProtectedSecret,
        password: &ProtectedSecret,
        otp: &str,
    ) -> Result<BwLoginSummary>;

    /// `verify-yubikey --check bw-login`（spec L155 / L201）の外部到達確認を行う。
    ///
    /// 設計は、login / unlock を実際に成立させずに `bw` CLI と Bitwarden Password Manager への到達性だけを
    /// 確認する外部 check として `--check bw-login` を定義する。implementor は `bw` CLI の利用可否と Bitwarden
    /// Password Manager への到達確認を行い、到達できない場合は `Err` を返す。secret 本文は要求しない。
    fn check_bw_login_reachable(&self) -> Result<()>;
}
