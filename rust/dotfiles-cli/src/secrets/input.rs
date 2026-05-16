//! `dotfiles secrets` の利用者入力を storage model へ変換する入力層。
//!
//! TTY 判定と stdout 書き込みは `util::terminal`、memory lock は `util::protection` に委譲し、
//! この層は prompt、stdin、JSON schema の入力形式と error contract を固定する。

use std::io::{self, Read, Write};

use anyhow::bail;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use secrecy::{ExposeSecret, SecretBox};
use serde::Deserialize;

use crate::Result;

#[cfg(test)]
use super::storage::{BootstrapSecretSource, SecretName};
use super::{
    application::ProtectedBootstrapSecrets,
    storage::SecretBytes,
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
    read_hidden_secret_line(prompt, limit, memory)?.into_protected_secret_line(
        memory,
        limit,
        "hidden secret input is too large",
    )
}

/// echo なしの prompt で YubiKey PIN を読み、保護 session に所属させる。
pub(crate) fn read_yubikey_pin<'session>(
    memory: &'session SecretSession,
) -> Result<Protected<'session, YubikeyPin>> {
    let pin = read_hidden_secret_line("YubiKey PIN: ", PIV_PIN_MAX_LEN, memory)?
        .into_protected_secret_line(memory, PIV_PIN_MAX_LEN, "YubiKey PIN is too long")?;
    let pin = pin.with_secret(|pin| YubikeyPin::new(pin.to_vec()))?;
    let lock = memory.lock_secret_value(&pin, YubikeyPin::memory_range)?;
    memory.protect_locked_value(pin, lock)
}

fn read_hidden_secret_line(
    prompt: &str,
    limit: usize,
    memory: &SecretSession,
) -> Result<ProtectedInputBuffer> {
    eprint!("{prompt}");
    io::stderr().flush()?;
    enable_raw_mode()?;
    let _raw_mode = scopeguard::guard((), |_| {
        let _ = disable_raw_mode();
    });
    let mut input = ProtectedInputBuffer::new(limit + 1, Some(memory))?;
    let mut stdin = io::stdin();
    let mut byte = [0u8; 1];
    loop {
        if stdin.read(&mut byte)? == 0 {
            return Ok(input);
        }
        match byte[0] {
            b'\r' | b'\n' => {
                eprintln!();
                return Ok(input);
            }
            3 => bail!("interrupted while reading hidden secret input"),
            8 | 127 => {
                input.pop();
            }
            value => {
                if input.as_slice().len() + 1 > limit {
                    bail!("hidden secret input is too large");
                }
                input.write_all(&[value])?;
            }
        }
    }
}

fn validate_yubikey_pin(pin: &[u8]) -> Result<()> {
    if !(PIV_PIN_MIN_LEN..=PIV_PIN_MAX_LEN).contains(&pin.len()) {
        bail!("YubiKey PIN must be 6 to 8 bytes");
    }
    Ok(())
}

pub(super) fn parse_protected_bootstrap_secrets_json<'session>(
    input: &[u8],
    field_limit: usize,
    memory: &'session SecretSession,
) -> Result<ProtectedBootstrapSecrets<'session>> {
    let input: BorrowedBootstrapSecretsInput<'_> = serde_json::from_slice(input)?;
    let bw_email = protected_json_field(input.bw_email, field_limit, memory, "bw-email")?;
    let bw_password = protected_json_field(input.bw_password, field_limit, memory, "bw-password")?;
    let bws_access_token = protected_json_field(
        input.bws_access_token,
        field_limit,
        memory,
        "bws-access-token",
    )?;
    Ok(ProtectedBootstrapSecrets::new(
        bw_email,
        bw_password,
        bws_access_token,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BorrowedBootstrapSecretsInput<'input> {
    #[serde(rename = "bw-email", borrow)]
    bw_email: &'input str,
    #[serde(rename = "bw-password", borrow)]
    bw_password: &'input str,
    #[serde(rename = "bws-access-token", borrow)]
    bws_access_token: &'input str,
}

fn protected_json_field<'session>(
    value: &str,
    field_limit: usize,
    memory: &'session SecretSession,
    name: &str,
) -> Result<Protected<'session, SecretBytes>> {
    if value.len() > field_limit {
        bail!("{name} is too large");
    }
    let input = ProtectedInputBuffer::from_slice(value.as_bytes(), Some(memory))?;
    input.into_protected_secret_line(memory, field_limit, "bootstrap secret field is too large")
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

    fn parse_test_bootstrap_json<'session>(
        input: &[u8],
        field_limit: usize,
        memory: &'session SecretSession,
    ) -> Result<ProtectedBootstrapSecrets<'session>> {
        parse_protected_bootstrap_secrets_json(input, field_limit, memory)
    }

    #[test]
    fn parse_bootstrap_secrets_json_accepts_expected_schema() -> Result<()> {
        let session = SecretSession::start()?;
        let secrets = parse_test_bootstrap_json(
            br#"{
                "bw-email": "alice@example.com",
                "bw-password": "password",
                "bws-access-token": "token"
            }"#,
            16 * 1024,
            &session,
        )?;

        secrets.with_secret(SecretName::BwEmail, |secret| {
            assert_eq!(secret, b"alice@example.com")
        });
        secrets.with_secret(SecretName::BwPassword, |secret| {
            assert_eq!(secret, b"password")
        });
        secrets.with_secret(SecretName::BwsAccessToken, |secret| {
            assert_eq!(secret, b"token");
        });
        Ok(())
    }

    #[test]
    fn parse_bootstrap_secrets_json_rejects_missing_field() -> Result<()> {
        let session = SecretSession::start()?;
        let result = parse_test_bootstrap_json(
            br#"{
                "bw-email": "alice@example.com",
                "bw-password": "password"
            }"#,
            16 * 1024,
            &session,
        );

        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn parse_bootstrap_secrets_json_rejects_duplicate_field() -> Result<()> {
        let session = SecretSession::start()?;
        let result = parse_test_bootstrap_json(
            br#"{
                "bw-email": "alice@example.com",
                "bw-email": "bob@example.com",
                "bw-password": "password",
                "bws-access-token": "token"
            }"#,
            16 * 1024,
            &session,
        );

        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn parse_bootstrap_secrets_json_rejects_non_string_field() -> Result<()> {
        let session = SecretSession::start()?;
        let result = parse_test_bootstrap_json(
            br#"{
                "bw-email": "alice@example.com",
                "bw-password": 123,
                "bws-access-token": "token"
            }"#,
            16 * 1024,
            &session,
        );

        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn parse_bootstrap_secrets_json_rejects_trailing_garbage() -> Result<()> {
        let session = SecretSession::start()?;
        let result = parse_test_bootstrap_json(
            br#"{
                "bw-email": "alice@example.com",
                "bw-password": "password",
                "bws-access-token": "token"
            } trailing"#,
            16 * 1024,
            &session,
        );

        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn parse_bootstrap_secrets_json_rejects_unknown_field() -> Result<()> {
        let session = SecretSession::start()?;
        let result = parse_test_bootstrap_json(
            br#"{
                "bw-email": "alice@example.com",
                "bw-password": "password",
                "bws-access-token": "token",
                "extra-secret": "ignored"
            }"#,
            16 * 1024,
            &session,
        );

        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn parse_bootstrap_secrets_json_rejects_field_past_limit() -> Result<()> {
        let session = SecretSession::start()?;
        let result = parse_test_bootstrap_json(
            br#"{
                "bw-email": "alice@example.com",
                "bw-password": "abcd",
                "bws-access-token": "token"
            }"#,
            3,
            &session,
        );

        assert!(result.is_err());
        Ok(())
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
