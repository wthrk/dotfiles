//! process / terminal / stdio / report 出力へ application が要求する port 契約。
//!
//! この module は入力取得、継続確認、secret 出力、report 出力の capability を宣言し、
//! prompt 文言や JSON 表現、端末制御の実装を adapter 側へ閉じる。

use std::collections::BTreeMap;

use super::super::{
    domain::{
        enrollment::EnrollSummary,
        gpg_restore::{OpenSshPublicKey, RestoreGpgSummary},
        verification::VerifySummary,
    },
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
    fn write_restore_gpg_report(&self, summary: &RestoreGpgSummary) -> Result<()>;
}

/// use case が gpg-secret-key-backup の上書き更新を明示確認する契約。
///
/// 設計「recipient 運用 / BWS 更新契約」は、recipient 追加を含む envelope 更新を対話実行では明示確認後に、
/// 非対話実行では明示的上書き許可 option がある場合だけ実行することを要求する。caller は確認に必要な
/// project/secret 名と primary fingerprint を渡し、`assume_overwrite` で非対話の明示許可有無を伝える。
/// implementor は TTY 可否を判定し、対話時は表示と回答取得を、非対話時は `assume_overwrite` の評価を担う。
#[cfg_attr(test, mockall::automock)]
pub trait BackupUpdateConfirmationPort {
    fn confirm_backup_update(
        &self,
        project_name: &str,
        secret_name: &str,
        primary_fingerprint: &str,
        assume_overwrite: bool,
    ) -> Result<bool>;
}

/// use case が backup envelope の `exported_at` 用に現在時刻を取得する契約。
///
/// 乱数と同様に時刻取得も外部依存であり、application が直接 system clock を読まないために port 化する。
/// caller は UTC RFC3339 timestamp を必要とするだけで、clock 実装や timezone を知らない。implementor は
/// wall-clock UTC を `YYYY-MM-DDThh:mm:ssZ` 形式の文字列として返す。
#[cfg_attr(test, mockall::automock)]
pub trait ClockPort {
    fn now_rfc3339_utc(&self) -> Result<String>;
}

/// use case が authentication subkey 由来の OpenSSH 公開鍵を出力境界へ渡す契約。
///
/// 公開鍵は秘密情報ではないため、`SecretOutputPort` とは別 capability として stdout へ機械可読な
/// 1 行を出力する。caller は domain 検証済みの公開鍵行を渡すだけで、書き込み方式を知らない。
/// implementor は terminal でも出力を許可し、GitHub API 呼び出しや鍵サーバー参照を内部で行わない。
#[cfg_attr(test, mockall::automock)]
pub trait SshPublicKeyOutputPort {
    fn write_ssh_public_key(&self, public_key: &OpenSshPublicKey) -> Result<()>;
}
