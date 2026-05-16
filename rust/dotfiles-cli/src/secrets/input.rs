//! `dotfiles secrets` の利用者入力を storage model へ変換する入力層。
//!
//! TTY 判定と stdout 書き込みは `util::terminal`、memory lock は `util::protection` に委譲し、
//! この層は prompt、stdin、JSON schema の入力形式と error contract を固定する。

use anyhow::bail;
use secrecy::{ExposeSecret, SecretBox};
use serde::Deserialize;

use crate::Result;

use super::{
    storage::{BootstrapSecrets, SecretBytes},
    util::{
        protection::{Protected, ProtectedInputBuffer, ProtectedSecretBytes, SecretSession},
        terminal,
    },
};

const PIV_PIN_MIN_LEN: usize = 6;
const PIV_PIN_MAX_LEN: usize = 8;

pub(crate) struct YubikeyPin(SecretBox<Vec<u8>>);

impl YubikeyPin {
    pub(crate) fn new(pin: Vec<u8>) -> Result<Self> {
        validate_yubikey_pin(&pin)?;
        Ok(Self(SecretBox::new(Box::new(pin))))
    }

    pub(crate) fn memory_range(&self) -> (*const u8, usize) {
        let pin = self.0.expose_secret();
        (pin.as_ptr(), pin.len())
    }
}

impl ProtectedSecretBytes for YubikeyPin {
    fn with_protected_bytes<R>(&self, borrow: impl FnOnce(&[u8]) -> R) -> R {
        borrow(self.0.expose_secret().as_slice())
    }
}

/// 表示 prompt で 1 行を読み、lock 済み入力 buffer として返す。
///
/// 末尾改行を除いた bytes に上限を適用する。
pub(super) fn read_visible_secret_line<'session>(
    prompt: &str,
    limit: usize,
    memory: &'session SecretSession,
) -> Result<Protected<'session, SecretBytes>> {
    use std::io::{self, Write};

    eprint!("{prompt}");
    io::stderr().flush()?;
    let input =
        ProtectedInputBuffer::read_line_until_newline_from(std::io::stdin(), limit, Some(memory))?;
    input.into_protected_secret_line(memory, limit, "visible secret input is too large")
}

/// echo なしの prompt で 1 行を読み、lock 済み入力 buffer として返す。
///
/// 読み込んだ bytes に上限を適用する。
pub(super) fn read_hidden_secret<'session>(
    prompt: &str,
    limit: usize,
    memory: &'session SecretSession,
) -> Result<Protected<'session, SecretBytes>> {
    let value = rpassword::prompt_password(prompt)?.into_bytes();
    if value.len() > limit {
        bail!("hidden secret input is too large");
    }
    let secret = SecretBytes::new(value);
    let lock = memory.lock_secret_value(&secret, SecretBytes::memory_range)?;
    memory.protect_locked_value(secret, lock)
}

/// echo なしの prompt で YubiKey PIN を読み、保護 session に所属させる。
pub(crate) fn read_yubikey_pin<'session>(
    memory: &'session SecretSession,
) -> Result<Protected<'session, YubikeyPin>> {
    let pin = YubikeyPin::new(rpassword::prompt_password("YubiKey PIN: ")?.into_bytes())?;
    let lock = memory.lock_secret_value(&pin, YubikeyPin::memory_range)?;
    memory.protect_locked_value(pin, lock)
}

fn validate_yubikey_pin(pin: &[u8]) -> Result<()> {
    if !(PIV_PIN_MIN_LEN..=PIV_PIN_MAX_LEN).contains(&pin.len()) {
        bail!("YubiKey PIN must be 6 to 8 bytes");
    }
    Ok(())
}

/// stdin の JSON bytes を bootstrap 登録用 schema として parse し、型付き model へ変換する。
pub(super) fn parse_bootstrap_secrets_json(input: &[u8]) -> Result<BootstrapSecrets> {
    let input: BootstrapSecretsInput = serde_json::from_slice(input)?;
    Ok(input.into_bootstrap_secrets())
}

#[derive(Deserialize)]
struct BootstrapSecretsInput {
    #[serde(rename = "bw-email")]
    bw_email: SecretBytes,
    #[serde(rename = "bw-password")]
    bw_password: SecretBytes,
    #[serde(rename = "bws-access-token")]
    bws_access_token: SecretBytes,
}

impl BootstrapSecretsInput {
    /// JSON 入力の 3 field を bootstrap 登録用 model へ所有権ごと移す。
    fn into_bootstrap_secrets(self) -> BootstrapSecrets {
        BootstrapSecrets {
            bw_email: self.bw_email,
            bw_password: self.bw_password,
            bws_access_token: self.bws_access_token,
        }
    }
}

/// stdout が TTY の場合は、復号結果を書き込む前に利用者向け error で停止する。
pub(crate) fn ensure_secret_stdout_not_terminal() -> Result<()> {
    if terminal::stdout_is_terminal() {
        reject_secret_stdout_terminal()?;
    }
    Ok(())
}

/// stdout の TTY 拒否を確認してから、復号済み bytes を stdout へ書き込む。
pub(crate) fn write_secret_to_stdout(bytes: &[u8]) -> Result<()> {
    ensure_secret_stdout_not_terminal()?;
    terminal::write_all_stdout(bytes)
}

/// stdout が TTY の場合に返す利用者向け error を、実プロセス境界と test 境界で共有する。
pub(crate) fn reject_secret_stdout_terminal() -> Result<()> {
    bail!("refusing to write secret to terminal; redirect stdout to a file or pipe");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bootstrap_secrets_json_accepts_expected_schema() -> Result<()> {
        let secrets = parse_bootstrap_secrets_json(
            br#"{
                "bw-email": "alice@example.com",
                "bw-password": "password",
                "bws-access-token": "token"
            }"#,
        )?;

        secrets
            .bw_email
            .with_secret(|secret| assert_eq!(secret, b"alice@example.com"));
        secrets
            .bw_password
            .with_secret(|secret| assert_eq!(secret, b"password"));
        secrets
            .bws_access_token
            .with_secret(|secret| assert_eq!(secret, b"token"));
        Ok(())
    }

    #[test]
    fn parse_bootstrap_secrets_json_rejects_missing_field() {
        let result = parse_bootstrap_secrets_json(
            br#"{
                "bw-email": "alice@example.com",
                "bw-password": "password"
            }"#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn parse_bootstrap_secrets_json_rejects_duplicate_field() {
        let result = parse_bootstrap_secrets_json(
            br#"{
                "bw-email": "alice@example.com",
                "bw-email": "bob@example.com",
                "bw-password": "password",
                "bws-access-token": "token"
            }"#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn parse_bootstrap_secrets_json_rejects_non_string_field() {
        let result = parse_bootstrap_secrets_json(
            br#"{
                "bw-email": "alice@example.com",
                "bw-password": 123,
                "bws-access-token": "token"
            }"#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn parse_bootstrap_secrets_json_rejects_trailing_garbage() {
        let result = parse_bootstrap_secrets_json(
            br#"{
                "bw-email": "alice@example.com",
                "bw-password": "password",
                "bws-access-token": "token"
            } trailing"#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn validate_yubikey_pin_accepts_piv_length_range() -> Result<()> {
        validate_yubikey_pin(b"123456")?;
        validate_yubikey_pin(b"12345678")?;
        Ok(())
    }

    #[test]
    fn validate_yubikey_pin_rejects_values_outside_piv_length_range() {
        assert!(validate_yubikey_pin(b"12345").is_err());
        assert!(validate_yubikey_pin(b"123456789").is_err());
    }
}
