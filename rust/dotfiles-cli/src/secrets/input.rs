//! `dotfiles secrets` の利用者入力を storage model へ変換する入力層。
//!
//! TTY 判定と stdout 書き込みは `util::terminal`、memory lock は `util::protection` に委譲し、
//! この層は prompt、stdin、JSON schema の入力形式と error contract を固定する。

use std::fmt;

use anyhow::bail;
use serde::{
    Deserialize,
    de::{self, DeserializeSeed, MapAccess, Visitor},
};

use crate::Result;

#[cfg(test)]
use super::storage::{BootstrapSecretSource, SecretName};
use super::{
    application::ProtectedBootstrapSecrets,
    util::{
        protection::{ProtectedInputBuffer, ProtectedSecret, SecretSession},
        terminal,
    },
};

const PIV_PIN_MIN_LEN: usize = 6;
const PIV_PIN_MAX_LEN: usize = 8;

/// 表示 prompt で 1 行を読み、lock 済み入力 buffer として返す。
///
/// 末尾改行を除いた bytes に上限を適用する。
pub(super) fn read_visible_secret_line<'session>(
    prompt: &str,
    limit: usize,
    memory: &'session SecretSession,
) -> Result<ProtectedSecret<'session>> {
    use std::io::{self, Write};

    eprint!("{prompt}");
    io::stderr().flush()?;
    let input =
        ProtectedInputBuffer::read_line_until_newline_from(std::io::stdin(), limit, memory)?;
    input.into_protected_secret_line(memory, limit, "visible secret input is too large")
}

/// echo なしの prompt で 1 行を読み、lock 済み入力 buffer として返す。
///
/// 読み込んだ bytes に上限を適用する。
pub(super) fn read_hidden_secret<'session>(
    prompt: &str,
    limit: usize,
    memory: &'session SecretSession,
) -> Result<ProtectedSecret<'session>> {
    terminal::read_hidden_input(prompt, limit, memory)?.into_protected_secret_line(
        memory,
        limit,
        "hidden secret input is too large",
    )
}

/// echo なしの prompt で YubiKey PIN を読み、保護 session に所属させる。
pub(crate) fn read_yubikey_pin<'session>(
    memory: &'session SecretSession,
) -> Result<ProtectedSecret<'session>> {
    let pin = terminal::read_hidden_input("YubiKey PIN: ", PIV_PIN_MAX_LEN, memory)?
        .into_protected_secret_line(memory, PIV_PIN_MAX_LEN, "YubiKey PIN is too long")?;
    pin.with_secret(validate_yubikey_pin)?;
    Ok(pin)
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
    let mut deserializer = serde_json::Deserializer::from_slice(input);
    let secrets = BootstrapSecretsSeed {
        field_limit,
        memory,
    }
    .deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(secrets)
}

#[derive(Deserialize)]
enum BootstrapSecretField {
    #[serde(rename = "bw-email")]
    BwEmail,
    #[serde(rename = "bw-password")]
    BwPassword,
    #[serde(rename = "bws-access-token")]
    BwsAccessToken,
}

impl BootstrapSecretField {
    fn name(&self) -> &'static str {
        match self {
            Self::BwEmail => "bw-email",
            Self::BwPassword => "bw-password",
            Self::BwsAccessToken => "bws-access-token",
        }
    }
}

struct BootstrapSecretsSeed<'session> {
    field_limit: usize,
    memory: &'session SecretSession,
}

impl<'de, 'session> DeserializeSeed<'de> for BootstrapSecretsSeed<'session> {
    type Value = ProtectedBootstrapSecrets<'session>;

    fn deserialize<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(BootstrapSecretsVisitor {
            field_limit: self.field_limit,
            memory: self.memory,
        })
    }
}

struct BootstrapSecretsVisitor<'session> {
    field_limit: usize,
    memory: &'session SecretSession,
}

impl<'de, 'session> Visitor<'de> for BootstrapSecretsVisitor<'session> {
    type Value = ProtectedBootstrapSecrets<'session>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bootstrap secrets object")
    }

    fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut bw_email = None;
        let mut bw_password = None;
        let mut bws_access_token = None;

        while let Some(field) = map.next_key::<BootstrapSecretField>()? {
            let target = match field {
                BootstrapSecretField::BwEmail => &mut bw_email,
                BootstrapSecretField::BwPassword => &mut bw_password,
                BootstrapSecretField::BwsAccessToken => &mut bws_access_token,
            };
            if target.is_some() {
                return Err(de::Error::duplicate_field(field.name()));
            }
            *target = Some(map.next_value_seed(ProtectedJsonFieldSeed {
                field_limit: self.field_limit,
                memory: self.memory,
            })?);
        }

        let bw_email = bw_email.ok_or_else(|| de::Error::missing_field("bw-email"))?;
        let bw_password = bw_password.ok_or_else(|| de::Error::missing_field("bw-password"))?;
        let bws_access_token =
            bws_access_token.ok_or_else(|| de::Error::missing_field("bws-access-token"))?;
        Ok(ProtectedBootstrapSecrets::new(
            bw_email,
            bw_password,
            bws_access_token,
        ))
    }
}

struct ProtectedJsonFieldSeed<'session> {
    field_limit: usize,
    memory: &'session SecretSession,
}

impl<'de, 'session> DeserializeSeed<'de> for ProtectedJsonFieldSeed<'session> {
    type Value = ProtectedSecret<'session>;

    fn deserialize<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = ProtectedInputBuffer::serde_string_seed(self.field_limit, self.memory)
            .deserialize(deserializer)?;
        input
            .into_protected_secret(self.memory)
            .map_err(de::Error::custom)
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
    fn parse_bootstrap_secrets_json_decodes_json_string_escapes() -> Result<()> {
        let session = SecretSession::start()?;
        let secrets = parse_test_bootstrap_json(
            br#"{
                "bw-email": "alice\u0040example.com",
                "bw-password": "line\nslash\\quote\"",
                "bws-access-token": "emoji-\uD83D\uDD11"
            }"#,
            16 * 1024,
            &session,
        )?;

        secrets.with_secret(SecretName::BwEmail, |secret| {
            assert_eq!(secret, b"alice@example.com")
        });
        secrets.with_secret(SecretName::BwPassword, |secret| {
            assert_eq!(secret, b"line\nslash\\quote\"")
        });
        secrets.with_secret(SecretName::BwsAccessToken, |secret| {
            assert_eq!(secret, b"emoji-\xf0\x9f\x94\x91");
        });
        Ok(())
    }

    #[test]
    fn parse_bootstrap_secrets_json_preserves_decoded_trailing_newline() -> Result<()> {
        let session = SecretSession::start()?;
        let secrets = parse_test_bootstrap_json(
            br#"{
                "bw-email": "alice@example.com\n",
                "bw-password": "password\n",
                "bws-access-token": "token\n"
            }"#,
            16 * 1024,
            &session,
        )?;

        secrets.with_secret(SecretName::BwEmail, |secret| {
            assert_eq!(secret, b"alice@example.com\n")
        });
        secrets.with_secret(SecretName::BwPassword, |secret| {
            assert_eq!(secret, b"password\n")
        });
        secrets.with_secret(SecretName::BwsAccessToken, |secret| {
            assert_eq!(secret, b"token\n");
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
