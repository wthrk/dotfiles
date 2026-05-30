//! process / terminal / stdio / report 出力へ application が要求する port 契約。
//!
//! この module は入力取得、継続確認、secret 出力、report 出力の capability を宣言し、
//! prompt 文言や JSON 表現、端末制御の実装を adapter 側へ閉じる。

use std::collections::BTreeMap;

use super::super::{
    domain::{enrollment::EnrollSummary, verification::VerifySummary},
    support::protection::ProtectedSecret,
};
use crate::Result;

/// use case が PIV PIN を取得するための capability 契約。
///
/// caller は PIN が必要な順序を決めるだけで、端末 echo 制御や buffer 保護を知らない。
/// implementor は入力取得と保護 backend 化を担い、PIN をログ・エラー・表示へ出さない。
#[cfg_attr(test, mockall::automock)]
pub trait PinInputPort {
    fn read_pin(&self) -> Result<ProtectedSecret>;
}

/// use case が必要とする secret 入力 capability 契約。
///
/// caller は必要な secret 種別または stream 入力 capability を明示して呼ぶ。implementor は prompt、
/// stdin、保護 buffer 化を外部 I/O 境界に閉じ、取得した平文を公開 API として返さない。
#[cfg_attr(test, mockall::automock)]
pub trait SecretInputPort {
    fn read_bw_email_secret(&self) -> Result<ProtectedSecret>;
    fn read_bw_password_secret(&self) -> Result<ProtectedSecret>;
    fn read_bws_access_token_secret(&self) -> Result<ProtectedSecret>;
    fn read_streamed_secret(&self) -> Result<ProtectedSecret>;
}

/// use case が対話 rotate の継続可否を外部入力から取得する capability 契約。
///
/// caller は継続確認が必要な地点だけを決める。implementor は TTY 可否と回答取得を扱い、
/// rotate 対象 serial や token 更新の業務判断を持たない。
#[cfg_attr(test, mockall::automock)]
pub trait RotationContinuationPort {
    fn continue_rotation(&self) -> Result<bool>;
}

/// bootstrap secret 文書を取得する capability 契約。
///
/// caller は bootstrap field map を要求するだけで JSON 入力手段や byte 上限を知らない。
/// implementor は入力 decode と secret backend 化を担い、wire/domain の妥当性判断は外へ漏らさない。
#[cfg_attr(test, mockall::automock)]
pub trait BootstrapSecretDocumentInputPort {
    fn read_bootstrap_secret_fields(&self) -> Result<BTreeMap<String, ProtectedSecret>>;
}

/// use case が復号済み secret を出力境界へ渡す契約。
///
/// caller は出力すべき secret material を渡すだけで、端末直書き拒否や stdout 書き込み方式を知らない。
/// implementor は安全な出力先判定を行い、secret を診断文脈へ混ぜない責務を負う。
#[cfg_attr(test, mockall::automock)]
pub trait SecretOutputPort {
    fn write_secret(&self, secret: &ProtectedSecret) -> Result<()>;
}

/// use case が結果報告を出力境界へ渡すための契約。
///
/// caller は domain summary の意味だけを渡す。implementor は JSON key、status 文字列、pretty
/// output など presentation 形式へ翻訳し、summary の成功条件を再定義しない。
#[cfg_attr(test, mockall::automock)]
pub trait ReportPort {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()>;
    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()>;
}
