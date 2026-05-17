//! `dotfiles secrets` の利用者入力を application の保護済み値へ変換する入力層。
//!
//! TTY 判定と stdout 書き込みは `support::terminal`、memory lock は `support::protection` に委譲し、
//! この層は prompt、stdin、JSON schema の入力形式と error contract を固定する。

use std::{
    borrow::Cow,
    fmt,
    io::{Read, Write},
};

use anyhow::{Context, bail};
use serde::{
    Deserialize,
    de::{self, DeserializeSeed, MapAccess, Visitor},
};

use crate::Result;

#[cfg(test)]
use super::domain::SecretName;
use super::{
    ports::EnrollmentSecretSet,
    support::{
        protection::{ProtectedInputBuffer, ProtectedSecret, SecretSession},
        terminal,
    },
};

const PIV_PIN_MIN_LEN: usize = 6;
const PIV_PIN_MAX_LEN: usize = 8;
pub(crate) const MAX_BOOTSTRAP_JSON_LEN: usize = 64 * 1024;
pub(crate) const MAX_SINGLE_STDIN_SECRET_LEN: usize = 16 * 1024;

/// stdin から 1 secret を読み、現在の session の保護済み値として返す。
///
/// 読み込み時の lock guard を引き継ぎ、unlock は値の破棄後に遅延させる。
pub(crate) fn read_protected_stdin_secret(
    limit: usize,
    session: &SecretSession,
) -> Result<ProtectedSecret<'_>> {
    if terminal::stdin_is_terminal() {
        bail!("--stdin requires pipe or redirect input");
    }
    let input = ProtectedInputBuffer::read_line_from(std::io::stdin(), limit, session)?;
    input.into_protected_secret_line(session, limit, "stdin secret input is too large")
}

/// 表示 prompt で 1 行を読み、lock 済み入力 buffer として返す。
///
/// 末尾改行を除いた bytes に上限を適用する。
pub(crate) fn read_visible_secret_line<'session>(
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
pub(crate) fn read_hidden_secret<'session>(
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

pub(crate) fn read_protected_enrollment_secret_set<'session>(
    reader: impl Read,
    input_limit: usize,
    field_limit: usize,
    memory: &'session SecretSession,
) -> Result<EnrollmentSecretSet<'session>> {
    let input = ProtectedInputBuffer::read_from(
        reader,
        input_limit,
        "bootstrap secret JSON input is too large",
        memory,
    )?;
    parse_protected_enrollment_secret_set_json(input.as_slice(), field_limit, memory)
        .context("failed to parse bootstrap secret JSON")
}

fn parse_protected_enrollment_secret_set_json<'session>(
    input: &[u8],
    field_limit: usize,
    memory: &'session SecretSession,
) -> Result<EnrollmentSecretSet<'session>> {
    let mut deserializer = serde_json::Deserializer::from_slice(input);
    let secrets = EnrollmentSecretSetSeed {
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

struct EnrollmentSecretSetSeed<'session> {
    field_limit: usize,
    memory: &'session SecretSession,
}

impl<'de, 'session> DeserializeSeed<'de> for EnrollmentSecretSetSeed<'session> {
    type Value = EnrollmentSecretSet<'session>;

    fn deserialize<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(EnrollmentSecretSetVisitor {
            field_limit: self.field_limit,
            memory: self.memory,
        })
    }
}

struct EnrollmentSecretSetVisitor<'session> {
    field_limit: usize,
    memory: &'session SecretSession,
}

impl<'de, 'session> Visitor<'de> for EnrollmentSecretSetVisitor<'session> {
    type Value = EnrollmentSecretSet<'session>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("enrollment secrets object")
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
        Ok(EnrollmentSecretSet::new(
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
        let value = Cow::<'de, str>::deserialize(deserializer)?;
        if value.len() > self.field_limit {
            return Err(de::Error::custom("protected input is too large"));
        }
        let mut input =
            ProtectedInputBuffer::new(value.len(), self.memory).map_err(de::Error::custom)?;
        input
            .write_all(value.as_bytes())
            .map_err(de::Error::custom)?;
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
    ) -> Result<EnrollmentSecretSet<'session>> {
        parse_protected_enrollment_secret_set_json(input, field_limit, memory)
    }

    #[test]
    fn parse_enrollment_secret_set_json_accepts_expected_schema() -> Result<()> {
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

        secrets.assert_secret_eq(SecretName::BwEmail, b"alice@example.com");
        secrets.assert_secret_eq(SecretName::BwPassword, b"password");
        secrets.assert_secret_eq(SecretName::BwsAccessToken, b"token");
        Ok(())
    }

    #[test]
    fn parse_enrollment_secret_set_json_decodes_json_string_escapes() -> Result<()> {
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
        secrets.assert_secret_eq(SecretName::BwEmail, b"alice@example.com");
        secrets.assert_secret_eq(SecretName::BwPassword, b"line\nslash\\quote\"");
        secrets.assert_secret_eq(SecretName::BwsAccessToken, "emoji-\u{1F511}".as_bytes());
        Ok(())
    }

    #[test]
    fn parse_enrollment_secret_set_json_keeps_decoded_trailing_newline() -> Result<()> {
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
        secrets.assert_secret_eq(SecretName::BwEmail, b"alice@example.com\n");
        secrets.assert_secret_eq(SecretName::BwPassword, b"password\n");
        secrets.assert_secret_eq(SecretName::BwsAccessToken, b"token\n");
        Ok(())
    }

    #[test]
    fn parse_enrollment_secret_set_json_rejects_missing_field() -> Result<()> {
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
    fn parse_enrollment_secret_set_json_rejects_duplicate_field() -> Result<()> {
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
    fn parse_enrollment_secret_set_json_rejects_non_string_field() -> Result<()> {
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
    fn parse_enrollment_secret_set_json_rejects_trailing_garbage() -> Result<()> {
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
    fn parse_enrollment_secret_set_json_rejects_unknown_field() -> Result<()> {
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
    fn parse_enrollment_secret_set_json_rejects_field_past_limit() -> Result<()> {
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
