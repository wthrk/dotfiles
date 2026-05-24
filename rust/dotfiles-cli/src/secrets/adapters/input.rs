//! `dotfiles secrets` の利用者入力 adapter。
//!
//! prompt、stdin、JSON schema、stdout の concrete な I/O 契約をここへ集め、application は
//! 保護済み値と非対話前提だけを扱う。

use std::{
    io::{self, Read, Write},
    sync::mpsc,
    thread,
    time::Duration,
};

use anyhow::{bail, Context};
use zeroize::Zeroize;

use crate::{
    secrets::{
        application::EnrollmentSecretSet,
        support::{
            protection::{ProtectedInputBuffer, ProtectedSecret, SecretSession},
            terminal,
        },
    },
    Result,
};

#[cfg(test)]
use crate::secrets::domain::SecretName;

const PIV_PIN_MIN_LEN: usize = 6;
const PIV_PIN_MAX_LEN: usize = 8;
pub(crate) const MAX_BOOTSTRAP_JSON_LEN: usize = 64 * 1024;
pub(crate) const MAX_SINGLE_STDIN_SECRET_LEN: usize = 16 * 1024;
pub(crate) const SECRET_STDOUT_TERMINAL_ERROR: &str =
    "refusing to write secret to terminal; redirect stdout to a file or pipe";

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
    eprint!("{prompt}");
    io::stderr().flush()?;
    let input = read_visible_secret_input(limit, memory)?;
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
    terminal::read_hidden_input(prompt, limit, "hidden secret input is too large", memory)?
        .into_protected_secret_line(memory, limit, "hidden secret input is too large")
}

/// echo なしの prompt で YubiKey PIN を読み、保護 session に所属させる。
pub(crate) fn read_yubikey_pin<'session>(
    memory: &'session SecretSession,
) -> Result<ProtectedSecret<'session>> {
    let pin = terminal::read_hidden_input(
        "YubiKey PIN: ",
        PIV_PIN_MAX_LEN,
        "YubiKey PIN is too long",
        memory,
    )?
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

/// 表示 prompt の 1 行入力を保護済み buffer へ直接積み、待機中は interrupt flag を監視する。
///
/// canonical mode の TTY 挙動を変えないよう raw mode には入らず、読み取り自体だけ worker thread に分離する。
fn read_visible_secret_input(limit: usize, memory: &SecretSession) -> Result<ProtectedInputBuffer> {
    let read_limit = limit + 3;
    let mut input = ProtectedInputBuffer::new(read_limit, memory)?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut stdin = io::stdin();
        let mut byte = [0u8; 1];
        loop {
            match stdin.read(&mut byte) {
                Ok(0) => {
                    let _ = sender.send(Ok(None));
                    break;
                }
                Ok(_) => {
                    let _ = sender.send(Ok(Some(byte[0])));
                    if byte[0] == b'\n' {
                        break;
                    }
                }
                Err(err) => {
                    let _ = sender.send(Err(err));
                    break;
                }
            }
        }
    });

    loop {
        if input.as_slice().len() >= read_limit {
            break;
        }
        memory.check_interrupted()?;
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(Some(byte))) => {
                input.write_all(&[byte])?;
                if byte == b'\n' {
                    break;
                }
            }
            Ok(Ok(None)) => break,
            Ok(Err(err)) => return Err(err.into()),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                bail!("failed to read terminal input")
            }
        }
    }

    Ok(input)
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
    EnrollmentSecretSetParser::new(input, field_limit, memory).parse()
}

enum BootstrapSecretField {
    BwEmail,
    BwPassword,
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

    fn from_decoded_key(key: &str) -> Option<Self> {
        match key {
            "bw-email" => Some(Self::BwEmail),
            "bw-password" => Some(Self::BwPassword),
            "bws-access-token" => Some(Self::BwsAccessToken),
            _ => None,
        }
    }
}

struct EnrollmentSecretSetParser<'input, 'session> {
    input: &'input [u8],
    cursor: usize,
    field_limit: usize,
    memory: &'session SecretSession,
}

impl<'input, 'session> EnrollmentSecretSetParser<'input, 'session> {
    fn new(input: &'input [u8], field_limit: usize, memory: &'session SecretSession) -> Self {
        Self {
            input,
            cursor: 0,
            field_limit,
            memory,
        }
    }

    fn parse(mut self) -> Result<EnrollmentSecretSet<'session>> {
        self.skip_whitespace();
        self.expect_byte(b'{')?;

        let mut bw_email = None;
        let mut bw_password = None;
        let mut bws_access_token = None;
        let mut first = true;
        loop {
            self.skip_whitespace();
            if self.peek_byte() == Some(b'}') {
                self.cursor += 1;
                break;
            }
            if !first {
                self.expect_byte(b',')?;
                self.skip_whitespace();
            }
            first = false;

            let key = self.parse_json_string_to_plaintext()?;
            let field = BootstrapSecretField::from_decoded_key(&key)
                .ok_or_else(|| anyhow::anyhow!("unknown field `{key}`"))?;
            self.skip_whitespace();
            self.expect_byte(b':')?;
            self.skip_whitespace();
            let secret = self.parse_json_string_to_protected_secret()?;
            let target = match field {
                BootstrapSecretField::BwEmail => &mut bw_email,
                BootstrapSecretField::BwPassword => &mut bw_password,
                BootstrapSecretField::BwsAccessToken => &mut bws_access_token,
            };
            if target.is_some() {
                bail!("duplicate field `{}`", field.name());
            }
            *target = Some(secret);
        }

        self.skip_whitespace();
        if self.cursor != self.input.len() {
            bail!("trailing characters after bootstrap secret JSON object");
        }

        let bw_email = bw_email.context("missing field `bw-email`")?;
        let bw_password = bw_password.context("missing field `bw-password`")?;
        let bws_access_token = bws_access_token.context("missing field `bws-access-token`")?;
        Ok(EnrollmentSecretSet::new(
            bw_email,
            bw_password,
            bws_access_token,
        ))
    }

    fn parse_json_string_to_plaintext(&mut self) -> Result<String> {
        let mut output = Vec::new();
        self.parse_json_string_into(|bytes| {
            output.extend_from_slice(bytes);
            Ok(())
        })?;
        String::from_utf8(output).context("JSON object key must be valid UTF-8")
    }

    fn parse_json_string_to_protected_secret(&mut self) -> Result<ProtectedSecret<'session>> {
        let field_limit = self.field_limit;
        let mut input = ProtectedInputBuffer::new(field_limit, self.memory)?;
        self.parse_json_string_into(|bytes| {
            let new_len = input.as_slice().len() + bytes.len();
            if new_len > field_limit {
                bail!("protected input is too large");
            }
            use std::io::Write;
            input.write_all(bytes)?;
            Ok(())
        })?;
        input.into_protected_secret(self.memory)
    }

    fn parse_json_string_into(
        &mut self,
        mut write_plaintext: impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<()> {
        self.expect_byte(b'"')?;
        while let Some(byte) = self.take_byte() {
            match byte {
                b'"' => return Ok(()),
                b'\\' => self.parse_escape(&mut write_plaintext)?,
                0x00..=0x1F => bail!("control character in JSON string"),
                0x20..=0x7F => write_plaintext(&[byte])?,
                utf8_head => self.parse_utf8_sequence(utf8_head, &mut write_plaintext)?,
            }
        }
        bail!("unterminated JSON string")
    }

    fn parse_utf8_sequence(
        &mut self,
        first_byte: u8,
        write_plaintext: &mut impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<()> {
        let sequence_len = match first_byte {
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => bail!("invalid UTF-8 in JSON string"),
        };
        let start = self.cursor - 1;
        let end = start + sequence_len;
        if end > self.input.len() {
            bail!("invalid UTF-8 in JSON string");
        }
        let sequence = &self.input[start..end];
        std::str::from_utf8(sequence).context("invalid UTF-8 in JSON string")?;
        self.cursor = end;
        write_plaintext(sequence)
    }

    fn parse_escape(
        &mut self,
        write_plaintext: &mut impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<()> {
        let escaped = self
            .take_byte()
            .ok_or_else(|| anyhow::anyhow!("unterminated escape sequence"))?;
        match escaped {
            b'"' => write_plaintext(b"\""),
            b'\\' => write_plaintext(b"\\"),
            b'/' => write_plaintext(b"/"),
            b'b' => write_plaintext(&[0x08]),
            b'f' => write_plaintext(&[0x0C]),
            b'n' => write_plaintext(b"\n"),
            b'r' => write_plaintext(b"\r"),
            b't' => write_plaintext(b"\t"),
            b'u' => self.parse_unicode_escape(write_plaintext),
            _ => bail!("invalid escape sequence in JSON string"),
        }
    }

    fn parse_unicode_escape(
        &mut self,
        write_plaintext: &mut impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<()> {
        let high = self.parse_hex_u16()?;
        let scalar = if (0xD800..=0xDBFF).contains(&high) {
            self.expect_byte(b'\\')?;
            self.expect_byte(b'u')?;
            let low = self.parse_hex_u16()?;
            if !(0xDC00..=0xDFFF).contains(&low) {
                bail!("invalid low surrogate in JSON string");
            }
            0x10000 + (((high as u32 - 0xD800) << 10) | (low as u32 - 0xDC00))
        } else if (0xDC00..=0xDFFF).contains(&high) {
            bail!("unexpected low surrogate in JSON string");
        } else {
            high as u32
        };
        let ch = char::from_u32(scalar).context("invalid unicode scalar value")?;
        let mut utf8 = [0u8; 4];
        let encoded_len = ch.encode_utf8(&mut utf8).len();
        let result = write_plaintext(&utf8[..encoded_len]);
        utf8.zeroize();
        result
    }

    fn parse_hex_u16(&mut self) -> Result<u16> {
        let mut value = 0u16;
        for _ in 0..4 {
            let byte = self
                .take_byte()
                .ok_or_else(|| anyhow::anyhow!("truncated unicode escape in JSON string"))?;
            value = (value << 4) | Self::hex_value(byte)?;
        }
        Ok(value)
    }

    fn hex_value(byte: u8) -> Result<u16> {
        match byte {
            b'0'..=b'9' => Ok((byte - b'0') as u16),
            b'a'..=b'f' => Ok((byte - b'a' + 10) as u16),
            b'A'..=b'F' => Ok((byte - b'A' + 10) as u16),
            _ => bail!("invalid hexadecimal digit in unicode escape"),
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek_byte(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.cursor += 1;
        }
    }

    fn expect_byte(&mut self, expected: u8) -> Result<()> {
        match self.take_byte() {
            Some(actual) if actual == expected => Ok(()),
            Some(_) => bail!("expected `{}` in JSON input", expected as char),
            None => bail!("unexpected end of JSON input"),
        }
    }

    fn peek_byte(&self) -> Option<u8> {
        self.input.get(self.cursor).copied()
    }

    fn take_byte(&mut self) -> Option<u8> {
        let byte = self.peek_byte()?;
        self.cursor += 1;
        Some(byte)
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
    bail!(SECRET_STDOUT_TERMINAL_ERROR);
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
}
