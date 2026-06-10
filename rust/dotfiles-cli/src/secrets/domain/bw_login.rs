//! Bitwarden Password Manager CLI login / unlock の入力値と結果意味を表す domain model。
//!
//! `bw` CLI へ渡す login email と YubiKey OTP は argv に載る非秘匿入力であり、master password
//! （`bw-password`）は子プロセスの `BW_PASSWORD` env でだけ渡す保護値である。この module は argv 値の
//! 妥当性（改行・制御文字・空文字の排除）と session 結果の意味だけを固定し、process 実行や env 注入の
//! 詳細は adapter / protection 境界へ閉じる。`bw` CLI の用途は spec L84 / L192 により login / unlock に限る。

use anyhow::Result;

use crate::secrets::support::protection::ProtectedSecret;

/// `bw login <email>` の argv に載せる Bitwarden login email。
///
/// `bw-email` は YubiKey に保存する値だが credential ではなく argv に載る非秘匿値である。argv へ
/// 安全に載せられるよう、空文字・改行・その他制御文字を含む値は domain rule として拒否する。さらに
/// `-` で始まる値は `bw login <email>` の positional 引数ではなく `bw` CLI の option として解釈され得る
/// ため、argv 安全性違反として拒否する。値のメール形式そのものは Bitwarden 側の責務であり、ここでは
/// argv 安全性だけを保証する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwLoginEmail(String);

impl BwLoginEmail {
    /// login email 文字列を argv 安全性の観点で検証して構築する。
    ///
    /// 空文字、前後空白だけ、改行・タブ・NUL を含む制御文字、先頭 `-` は拒否する。受理した値は前後
    /// 空白を取り除いた 1 行であり、`bw login <email>` の argv 引数として使える。
    pub fn parse(value: &str) -> Result<Self> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            anyhow::bail!("bw-email must not be empty");
        }
        if trimmed.chars().any(char::is_control) {
            anyhow::bail!("bw-email must not contain control characters");
        }
        if trimmed.starts_with('-') {
            anyhow::bail!("bw-email must not start with '-'");
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// argv に載せる検証済み email 文字列を返す。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// YubiKey storage から取得した `bw-email` を argv 安全な login email へ変換する。
    pub fn parse_protected(value: &ProtectedSecret) -> Result<Self> {
        value.with_secret_utf8(Self::parse)
    }
}

/// `bw login --code <otp>` の argv に載せる YubiKey OTP。
///
/// OTP は touch 生成・単回利用であり、argv に載せる前提の入力（spec L178）。可視入力で読んだ生文字列を
/// argv 安全性の観点で検証し、空文字・制御文字を拒否する。OTP は秘密の永続値ではなく単回トークンのため
/// 保護 buffer 化は要さないが、ログ・診断には残さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwOtp(String);

impl BwOtp {
    /// OTP 文字列を argv 安全性の観点で検証して構築する。
    pub fn parse(value: &str) -> Result<Self> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            anyhow::bail!("YubiKey OTP must not be empty");
        }
        if trimmed.chars().any(char::is_control) {
            anyhow::bail!("YubiKey OTP must not contain control characters");
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// argv に載せる検証済み OTP 文字列を返す。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// `bw login --method 3` が要求する YubiKey OTP の 2FA method 番号。
///
/// Bitwarden CLI の 2FA method 識別子で、YubiKey OTP は `3`（spec L178）。argv 値の固定は domain rule とし、
/// adapter で magic number を再定義しない。real `bw` CLI adapter（login_adapter）だけが argv へ載せるため、
/// `bw` CLI を起動しない stub build では未使用になる。
#[cfg_attr(feature = "secrets-internal-test-stub", expect(dead_code))]
pub const BW_OTP_TWO_FACTOR_METHOD: &str = "3";

/// `bw unlock --raw` が stdout に出力する session key を表す結果値。
///
/// `BW_SESSION` の値であり、後続の vault 操作へ完全アクセスを与える session credential である。spec L86 の
/// 「`BW_SESSION` の扱いは bw-login のコマンド仕様で定義する」に従い、この値は disk / dotfile へ永続化せず、
/// 利用者が自分で `export BW_SESSION=...` できるよう surface するためだけに保持する。空文字は unlock 失敗
/// として拒否し、改行・タブ・NUL 等の制御文字を含む値は不正な session として拒否する（`BwLoginEmail` /
/// `BwOtp` と同方針）。`'`（single-quote）は base64 系 session key には現れず、shell export 形式の整形は
/// presentation 責務として report 側の POSIX エスケープ（`shell_single_quote`）で安全化するため domain では
/// 拒否しない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwSessionKey(String);

impl BwSessionKey {
    /// `bw unlock --raw` の stdout を session key として検証して構築する。
    ///
    /// 空文字・前後空白だけの値は unlock 失敗として拒否し、制御文字（改行・CR・タブ・NUL 等）を含む値は
    /// 不正な session として拒否する。制御文字拒否は単一行制約を包含するが、改行・CR を明示拒否する単一行
    /// チェックは契約意図を示すため残す。受理した値は前後空白を取り除いた 1 行である。
    pub fn parse(value: &str) -> Result<Self> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            anyhow::bail!("bw unlock did not return a session key");
        }
        if trimmed.chars().any(|c| c == '\n' || c == '\r') {
            anyhow::bail!("bw session key must be a single line");
        }
        if trimmed.chars().any(char::is_control) {
            anyhow::bail!("bw session key must not contain control characters");
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// surface する session key 文字列を返す。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// bw-login use case の結果要約。
///
/// login / unlock が完了したことと、利用者へ surface する `BW_SESSION` の値だけを保持する。disk 永続化や
/// dotfile 書き込みの意味は持たず、表示文言は report 層が決める。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwLoginSummary {
    pub session: BwSessionKey,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_rejects_empty_and_control_characters() {
        assert!(BwLoginEmail::parse("  ").is_err());
        assert!(BwLoginEmail::parse("user@example.com\nINJECT").is_err());
        assert!(BwLoginEmail::parse("user\t@example.com").is_err());
        assert!(BwLoginEmail::parse("-inject@example.com").is_err());
        assert!(BwLoginEmail::parse("--apikey").is_err());
        let email = BwLoginEmail::parse("  user@example.com  ").expect("valid email");
        assert_eq!(email.as_str(), "user@example.com");
    }

    #[test]
    fn otp_rejects_empty_and_control_characters() {
        assert!(BwOtp::parse("").is_err());
        assert!(BwOtp::parse("ccccc\nbad").is_err());
        let otp = BwOtp::parse("  cccccbtdv....  ").expect("valid otp");
        assert_eq!(otp.as_str(), "cccccbtdv....");
    }

    #[test]
    fn session_key_rejects_empty_multiline_and_control_characters() {
        assert!(BwSessionKey::parse("   ").is_err());
        assert!(BwSessionKey::parse("line1\nline2").is_err());
        assert!(BwSessionKey::parse("abc\tdef").is_err());
        assert!(BwSessionKey::parse("abc\0def").is_err());
        let session = BwSessionKey::parse("  SESSIONKEY==  ").expect("valid session");
        assert_eq!(session.as_str(), "SESSIONKEY==");
    }
}
