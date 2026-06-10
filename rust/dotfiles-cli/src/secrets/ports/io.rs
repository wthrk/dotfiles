//! process / terminal / stdio / report 出力へ application が要求する port 契約。
//!
//! この module は入力取得、継続確認、secret 出力、report 出力の capability を宣言し、
//! prompt 文言や JSON 表現、端末制御の実装を adapter 側へ閉じる。

use super::super::{
    domain::{
        bw_login::BwLoginSummary,
        enrollment::EnrollSummary,
        gpg_restore::{OpenSshPublicKey, RestoreGpgSummary},
        pass_restore::RestorePassSummary,
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

/// use case が BWS 登録用 access token を保護値として取得する capability 契約。
///
/// provisioning command は YubiKey storage を読まず、この capability で受け取った BWS access token を
/// project / secret の create/use にだけ使う。この token は YubiKey へ保存しない。YubiKey の
/// `bws-access-token` は復旧時の read 用最小権限 token を別経路で保存するものであり、caller は同一値運用を
/// 前提にしてはならない。BWS access token は実 credential であり secret として扱う。caller は token を
/// 必要とする地点だけを決める。implementor は hidden prompt（TTY）または pipe（stdin）から保護 buffer へ
/// 読み込み、取得した平文を公開 API として返さず、argv・ログ・shell history・永続環境変数・永続一時ファイルへ
/// 残さない。
#[cfg_attr(test, mockall::automock)]
pub trait BwsAccessTokenInputPort {
    fn read_bws_access_token_for_provisioning(&self) -> Result<ProtectedSecret>;
}

/// use case が `password-store-remote` の clone URL を非秘匿入力として取得する capability 契約。
///
/// `password-store-remote` は private `password-store` repository の SSH clone URL であり、秘密情報では
/// ない。よって他の secret 入力（`SecretInputPort`）と異なり保護 buffer・非表示入力・zeroize を要さず、
/// caller は必要な地点でこの port に URL 取得を要求する。implementor は configured origin が無い場合だけ
/// controlling TTY の可視対話入力で取得した生文字列を返し、stdin pipe / argv / 環境変数の値中継は受け持たない。
/// URL 形式の妥当性判断（`git@github.com:<owner>/<repo>.git`）は domain rule に委ね、implementor は再定義しない。
#[cfg_attr(test, mockall::automock)]
pub trait PasswordStoreRemoteInputPort {
    fn read_password_store_remote_url(&self) -> Result<String>;
}

/// use case が YubiKey OTP を非秘匿入力として取得する capability 契約。
///
/// YubiKey OTP は touch 生成・単回利用のトークンで、`bw login --code <otp>` の argv に載る前提（spec L178）。
/// よって他の secret 入力（`SecretInputPort`）と異なり保護 buffer・非表示入力・zeroize を要さず、可視入力で
/// 1 行を読む。caller は OTP を必要とする地点だけを決める。implementor は stdin が terminal のとき可視プロンプト
/// （入力をエコーする通常入力）で、非 terminal（pipe）のとき stdin 1 行を読み、取得した生文字列を返す。OTP の
/// 妥当性判断（空文字・制御文字の排除）は domain rule に委ね、implementor は再定義しない。OTP は単回トークンの
/// ため永続化しないが、ログ・診断にも残さない。
#[cfg_attr(test, mockall::automock)]
pub trait BwOtpInputPort {
    fn read_bw_otp(&self) -> Result<String>;
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
    fn write_restore_pass_report(&self, summary: &RestorePassSummary) -> Result<()>;

    /// bw-login の結果（surface する `BW_SESSION` 値を含む）を出力境界へ渡す。
    ///
    /// caller は domain summary の意味だけを渡す。implementor は利用者が `export BW_SESSION=...` できる形で
    /// session 値を surface し、disk / dotfile へ永続化しない。master password は決して出力しない。
    fn write_bw_login_report(&self, summary: &BwLoginSummary) -> Result<()>;
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
