//! 実プロセス I/O と実機/stub backend を `SecretsBoundary` へ接続する adapter。
//!
//! stdin/stdout/terminal prompt・enrollment JSON decode・device backend 選択・device selection
//! prompt をすべてこの 1 ファイルに集約し、application 本体は順序制御だけに集中させる。

use std::{
    fs::OpenOptions,
    io::{self, IsTerminal, Read, Write},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use ::yubikey::{Context as YkContext, Serial, YubiKey};
use ::yubikey::Error as YkError;

use anyhow::{bail, Context};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use zeroize::{Zeroize, Zeroizing};

#[cfg(feature = "secrets-test-stub")]
use super::test_stub;
use super::yubikey;
use crate::{
    secrets::{
        ports::{EnrollmentBytes, SecretDevice, SecretsBoundary},
        support::protection::{InterruptGuard, ProtectedInputBuffer, ProtectedSecret, SecretSession},
    },
    Result,
};
#[cfg(feature = "secrets-test-stub")]
use crate::secrets::domain;

// ── device backend ────────────────────────────────────────────────────────────

#[cfg(feature = "secrets-test-stub")]
use dotfiles_cli_secrets_test_contract::{PRIMARY_SERIAL, SPARE_SERIAL};

#[cfg(feature = "secrets-test-stub")]
/// CLI 実行で使う YubiKey device adapter の選択状態。
///
/// application はこの値を保持するだけで、実機か stub かに応じた別 use case を持たない。
enum DeviceBackend {
    /// 実機 YubiKey adapter を使う通常実行。
    Real,
    /// CLI 統合テスト用の in-memory device adapter を使う実行。
    ///
    /// `next_interactive_serial` は serial 指定なし device 選択で次に使う serial を追跡する。
    TestStub { next_interactive_serial: u32 },
}

#[cfg(not(feature = "secrets-test-stub"))]
#[derive(Clone, Copy)]
/// CLI 実行で使う YubiKey device adapter の選択状態。
///
/// 通常 build では実機 adapter だけを持ち、stub 用の実行経路を含めない。
enum DeviceBackend {
    /// 実機 YubiKey adapter を使う通常実行。
    Real,
}

impl DeviceBackend {
    #[cfg(feature = "secrets-test-stub")]
    /// CLI option から device adapter の選択状態を構築する。
    ///
    /// `secrets-test-stub` feature 有効時だけ hidden test flag を解釈する。
    fn from_test_flag(enabled: bool) -> Result<Self> {
        if enabled {
            return Ok(Self::TestStub {
                next_interactive_serial: PRIMARY_SERIAL,
            });
        }
        Ok(Self::Real)
    }

    #[cfg(not(feature = "secrets-test-stub"))]
    /// 通常 build で実機 adapter の選択状態を構築する。
    fn from_test_flag(_enabled: bool) -> Result<Self> {
        Ok(Self::Real)
    }
}

// ── combined device type ──────────────────────────────────────────────────────

#[cfg(feature = "secrets-test-stub")]
/// 実機 YubiKey と device stub を同じ `SecretDevice` port として扱う adapter。
///
/// `secrets-test-stub` feature でだけ enum になり、application の use case は variant を見ない。
pub(super) enum YubikeySecretDevice {
    /// 実機 YubiKey の PIV device adapter。
    Real(yubikey::YubikeySecretDevice),
    /// CLI 統合テスト用の in-memory PIV device adapter。
    TestStub(test_stub::TestDevice),
}

#[cfg(not(feature = "secrets-test-stub"))]
/// 通常 build で application が扱う YubiKey device adapter。
pub(super) type YubikeySecretDevice = yubikey::YubikeySecretDevice;

#[cfg(feature = "secrets-test-stub")]
impl SecretDevice for YubikeySecretDevice {
    fn serial(&self) -> u32 {
        match self {
            Self::Real(device) => device.serial(),
            Self::TestStub(device) => device.serial(),
        }
    }

    fn key_exists(&mut self) -> Result<bool> {
        match self {
            Self::Real(device) => device.key_exists(),
            Self::TestStub(device) => device.key_exists(),
        }
    }

    fn check_key_generation_preconditions(&mut self) -> Result<()> {
        match self {
            Self::Real(device) => device.check_key_generation_preconditions(),
            Self::TestStub(device) => device.check_key_generation_preconditions(),
        }
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        match self {
            Self::Real(device) => device.check_management_auth_preconditions(),
            Self::TestStub(device) => device.check_management_auth_preconditions(),
        }
    }

    fn generate_key(&mut self) -> Result<()> {
        match self {
            Self::Real(device) => device.generate_key(),
            Self::TestStub(device) => device.generate_key(),
        }
    }

    fn read_object(&mut self, object_id: domain::PivObjectId) -> Result<Option<Vec<u8>>> {
        match self {
            Self::Real(device) => device.read_object(object_id),
            Self::TestStub(device) => device.read_object(object_id),
        }
    }

    fn write_object(&mut self, object_id: domain::PivObjectId, value: &mut [u8]) -> Result<()> {
        match self {
            Self::Real(device) => device.write_object(object_id, value),
            Self::TestStub(device) => device.write_object(object_id, value),
        }
    }

    fn wrap_key(&mut self, key: &[u8]) -> Result<Vec<u8>> {
        match self {
            Self::Real(device) => device.wrap_key(key),
            Self::TestStub(device) => device.wrap_key(key),
        }
    }

    fn verify_pin(&mut self, pin: &[u8]) -> Result<()> {
        match self {
            Self::Real(device) => device.verify_pin(pin),
            Self::TestStub(device) => device.verify_pin(pin),
        }
    }

    fn requires_pin_input(&self) -> bool {
        match self {
            Self::Real(device) => device.requires_pin_input(),
            Self::TestStub(device) => device.requires_pin_input(),
        }
    }

    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        match self {
            Self::Real(device) => device.unwrap_key(wrapped_key),
            Self::TestStub(device) => device.unwrap_key(wrapped_key),
        }
    }
}

// ── terminal I/O helpers ──────────────────────────────────────────────────────

/// 現在の stdin が対話入力を読める TTY かを返す。
fn stdin_is_terminal() -> bool {
    io::stdin().is_terminal()
}

/// 現在の stdout が画面表示される TTY かを返す。
fn stdout_is_terminal() -> bool {
    io::stdout().is_terminal()
}

/// TTY では prompt を stderr へ表示し、stdin の 1 行を yes/no 応答として返す。
///
/// stdin が TTY でない場合は入力を読まずに `false` を返す。
fn prompt_yes_no(prompt: &str, interrupt: &InterruptGuard) -> Result<bool> {
    if !stdin_is_terminal() {
        return Ok(false);
    }

    eprint!("{prompt}");
    io::stderr().flush()?;
    let input = read_terminal_line_interruptible(interrupt)?;
    Ok(matches!(
        input.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// TTY で Enter 入力を待ち、入力完了、期限切れ、中断のいずれかを返す。
///
/// stdin が TTY でない場合と deadline 超過時の error 文言は呼び出し側が指定する。
fn wait_for_enter(
    deadline: Instant,
    interrupt: &InterruptGuard,
    noninteractive_error: &'static str,
    timeout_error: &'static str,
) -> Result<()> {
    if !stdin_is_terminal() {
        bail!(noninteractive_error);
    }
    read_terminal_line_until(deadline, interrupt, timeout_error).map(|_| ())
}

/// byte 列を stdout へそのまま書き込む。
fn write_all_stdout(bytes: &[u8]) -> Result<()> {
    io::stdout().lock().write_all(bytes)?;
    Ok(())
}

/// echo せずに TTY から 1 行を読み、保護済み入力 buffer へ保持する。
///
/// 入力 bytes は `SecretSession` の memory lock 範囲へ直接書き込み、Enter で確定する。
/// stdin が pipe の場合は controlling terminal を開き、secret payload 用 stdin を消費しない。
fn read_hidden_input(
    prompt: &str,
    limit: usize,
    limit_error: &'static str,
    session: &SecretSession,
) -> Result<ProtectedInputBuffer> {
    eprint!("{prompt}");
    io::stderr().flush()?;

    let mut reader = hidden_input_reader()?;
    enable_raw_mode().context("failed to enable terminal raw mode")?;
    let _raw_mode = scopeguard::guard((), |_| {
        let _ = disable_raw_mode();
    });
    let mut input = ProtectedInputBuffer::new(limit + 1, session)?;
    let mut byte = [0u8; 1];
    loop {
        if reader.read(&mut byte)? == 0 {
            return Ok(input);
        }

        match byte[0] {
            b'\r' | b'\n' => {
                eprintln!();
                return Ok(input);
            }
            3 => {
                bail!("interrupted while reading hidden input");
            }
            8 | 127 => {
                input.pop_byte();
            }
            value => {
                input.write_all(&[value])?;
                if input.as_slice().len() > limit {
                    bail!(limit_error);
                }
            }
        }
    }
}

/// `stdin.read_line` の EOF 契約を保ったまま、interrupt flag を監視して 1 行入力を返す。
///
/// 標準入出力には portable な非同期 cancel API がないため、読み取り自体は worker thread に任せ、
/// 呼び出し側は一定間隔で interrupt flag を確認する。
fn read_terminal_line_interruptible(interrupt: &InterruptGuard) -> Result<String> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut input = String::new();
        let result = io::stdin().read_line(&mut input).map(|_| input);
        let _ = sender.send(result);
    });

    loop {
        interrupt.check_interrupted()?;
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(result) => return Ok(result?),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                bail!("failed to read terminal input")
            }
        }
    }
}

/// hidden prompt の入力元を、stdin または controlling terminal として開く。
///
/// stdin payload と PIN / hidden prompt を併用する経路では `/dev/tty` を使う。
fn hidden_input_reader() -> Result<Box<dyn io::Read>> {
    if stdin_is_terminal() {
        return Ok(Box::new(io::stdin()));
    }

    let tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .context("failed to open controlling terminal")?;
    Ok(Box::new(tty))
}

/// raw mode で 1 行を読み、Enter で入力を確定してそれまでの文字列を返す。
///
/// Ctrl-C、中断 flag、deadline 超過を同じ loop で監視し、deadline 超過時の error 文言は
/// 呼び出し側が指定する。
fn read_terminal_line_until(
    deadline: Instant,
    interrupt: &InterruptGuard,
    timeout_error: &'static str,
) -> Result<String> {
    enable_raw_mode().context("failed to enable terminal raw mode")?;
    let _raw_mode = scopeguard::guard((), |_| {
        let _ = disable_raw_mode();
    });
    let mut line = String::new();
    loop {
        interrupt.check_interrupted()?;
        let now = Instant::now();
        if now >= deadline {
            bail!(timeout_error);
        }
        let timeout = Duration::from_millis(100).min(deadline.saturating_duration_since(now));

        if !event::poll(timeout)? {
            continue;
        }

        if let Some(completed) = read_terminal_key_event(&mut line)? {
            return Ok(completed);
        }
    }
}

fn read_terminal_key_event(line: &mut String) -> Result<Option<String>> {
    let Event::Key(key) = event::read()? else {
        return Ok(None);
    };
    if key.kind != KeyEventKind::Press {
        return Ok(None);
    }

    match key.code {
        KeyCode::Enter => {
            eprintln!();
            return Ok(Some(line.clone()));
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            bail!("interrupted while reading terminal input");
        }
        KeyCode::Char(ch) => {
            line.push(ch);
            eprint!("{ch}");
            io::stderr().flush()?;
        }
        KeyCode::Backspace => {
            if line.pop().is_some() {
                eprint!("\u{8} \u{8}");
                io::stderr().flush()?;
            }
        }
        _ => {}
    }
    Ok(None)
}

// ── prompt helpers ────────────────────────────────────────────────────────────

const PIV_PIN_MIN_LEN: usize = 6;
const PIV_PIN_MAX_LEN: usize = 8;

/// 表示 prompt で 1 行を読み、zeroize 保護済み bytes として返す。
///
/// 末尾改行を除いた bytes に上限を適用する。
fn read_visible_line_bytes(prompt: &str, limit: usize) -> Result<Zeroizing<Vec<u8>>> {
    let session = SecretSession::start()?;
    eprint!("{prompt}");
    io::stderr().flush()?;
    let input = read_visible_secret_input(limit, &session)?;
    let protected =
        input.into_protected_secret_line(&session, limit, "visible secret input is too large")?;
    Ok(Zeroizing::new(protected.with_secret(|b| b.to_vec())))
}

/// echo なしの prompt で 1 行を読み、zeroize 保護済み bytes として返す。
///
/// 読み込んだ bytes に上限を適用する。
fn read_hidden_bytes(prompt: &str, limit: usize) -> Result<Zeroizing<Vec<u8>>> {
    let session = SecretSession::start()?;
    let protected =
        read_hidden_input(prompt, limit, "hidden secret input is too large", &session)?
            .into_protected_secret_line(&session, limit, "hidden secret input is too large")?;
    Ok(Zeroizing::new(protected.with_secret(|b| b.to_vec())))
}

/// echo なしの prompt で YubiKey PIN を読み、zeroize 保護済み bytes として返す。
fn read_yubikey_pin_raw() -> Result<Zeroizing<Vec<u8>>> {
    let session = SecretSession::start()?;
    let pin = read_hidden_input(
        "YubiKey PIN: ",
        PIV_PIN_MAX_LEN,
        "YubiKey PIN is too long",
        &session,
    )?
    .into_protected_secret_line(&session, PIV_PIN_MAX_LEN, "YubiKey PIN is too long")?;
    pin.with_secret(validate_yubikey_pin)?;
    Ok(Zeroizing::new(pin.with_secret(|b| b.to_vec())))
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

// ── stdin reader ──────────────────────────────────────────────────────────────

/// stdin から 1 secret を読み、zeroize 保護済み bytes として返す。
///
/// stdin が TTY の場合は error で失敗する。
fn read_stdin_bytes(limit: usize) -> Result<Zeroizing<Vec<u8>>> {
    if stdin_is_terminal() {
        bail!("--stdin requires pipe or redirect input");
    }
    let session = SecretSession::start()?;
    let input = ProtectedInputBuffer::read_line_from(std::io::stdin(), limit, &session)?;
    let protected =
        input.into_protected_secret_line(&session, limit, "stdin secret input is too large")?;
    Ok(Zeroizing::new(protected.with_secret(|b| b.to_vec())))
}

// ── stdout writer ─────────────────────────────────────────────────────────────

const SECRET_STDOUT_TERMINAL_ERROR: &str =
    "refusing to write secret to terminal; redirect stdout to a file or pipe";

/// stdout が TTY の場合は、復号結果を書き込む前に利用者向け error で停止する。
fn ensure_secret_stdout_not_terminal() -> Result<()> {
    if stdout_is_terminal() {
        bail!(SECRET_STDOUT_TERMINAL_ERROR);
    }
    Ok(())
}

/// stdout の TTY 拒否を確認してから、復号済み bytes を stdout へ書き込む。
fn write_secret_to_stdout(bytes: &[u8]) -> Result<()> {
    ensure_secret_stdout_not_terminal()?;
    write_all_stdout(bytes)
}

// ── enrollment JSON decoder ───────────────────────────────────────────────────

/// stdin JSON から enrollment secret の raw bytes を読み出す。
///
/// JSON をデコードして各フィールドの値を `Zeroizing<Vec<u8>>` として返す。
fn read_enrollment_json_bytes(
    reader: impl Read,
    input_limit: usize,
    field_limit: usize,
) -> Result<EnrollmentBytes> {
    let session = SecretSession::start()?;
    let input = ProtectedInputBuffer::read_from(
        reader,
        input_limit,
        "bootstrap secret JSON input is too large",
        &session,
    )?;
    EnrollmentSecretSetParser::new(input.as_slice(), field_limit, &session)
        .parse_to_bytes()
        .context("failed to parse bootstrap secret JSON")
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

    fn parse_to_bytes(mut self) -> Result<EnrollmentBytes> {
        self.skip_whitespace();
        self.expect_byte(b'{')?;

        let mut bw_email: Option<ProtectedSecret<'session>> = None;
        let mut bw_password: Option<ProtectedSecret<'session>> = None;
        let mut bws_access_token: Option<ProtectedSecret<'session>> = None;
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
        Ok(EnrollmentBytes {
            bw_email: Zeroizing::new(bw_email.with_secret(|b| b.to_vec())),
            bw_password: Zeroizing::new(bw_password.with_secret(|b| b.to_vec())),
            bws_access_token: Zeroizing::new(bws_access_token.with_secret(|b| b.to_vec())),
        })
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

// ── device selection prompt ───────────────────────────────────────────────────

/// reader 名と serial を持つ YubiKey 選択候補。
struct YubikeyCandidate<'a> {
    reader: &'a str,
    serial: u32,
}

/// 複数の YubiKey 候補を表示し、利用者が選んだ index を返す。
///
/// 非対話実行の判定は caller 側で完了してから呼ばれるため、この関数は候補表示と番号入力だけを扱う。
fn select_yubikey_candidate(
    candidates: &[YubikeyCandidate<'_>],
    timed_input: Option<(Instant, &InterruptGuard)>,
) -> Result<usize> {
    eprintln!("Select YubiKey:");
    for (index, candidate) in candidates.iter().enumerate() {
        eprintln!(
            "{}: serial {} ({})",
            index + 1,
            candidate.serial,
            candidate.reader
        );
    }
    eprint!("number: ");
    io::Write::flush(&mut io::stderr())?;

    let input = if let Some((deadline, interrupt)) = timed_input {
        read_terminal_line_until(
            deadline,
            interrupt,
            "timed out waiting for spare YubiKey",
        )?
    } else {
        let interrupt = InterruptGuard::install()
            .context("failed to install interrupt handler for YubiKey selection")?;
        read_terminal_line_interruptible(&interrupt)?
    };
    let selected = input
        .trim()
        .parse::<usize>()
        .map_err(anyhow::Error::from)
        .context("invalid selection")?;
    if selected == 0 || selected > candidates.len() {
        anyhow::bail!("selected YubiKey is out of range");
    }
    Ok(selected - 1)
}

/// primary と同じ device が選ばれた後、spare への差し替え完了を Enter で待つ。
///
/// 待機は spare 登録の deadline と interrupt policy に従う。
fn wait_for_spare_replacement(deadline: Instant, interrupt: &InterruptGuard) -> Result<()> {
    eprintln!("The selected YubiKey is the primary; replace it with the spare.");
    eprintln!("Insert the spare YubiKey, then press Enter.");
    wait_for_enter(
        deadline,
        interrupt,
        "cannot wait for spare YubiKey replacement without a controlling terminal",
        "timed out waiting for spare YubiKey",
    )
}

// ── YubiKey discovery helpers ─────────────────────────────────────────────────

const SPARE_WAIT_TIMEOUT: Duration = Duration::from_secs(300);
const SPARE_DETECT_POLL_INTERVAL: Duration = Duration::from_millis(200);
const SPARE_WAIT_TIMEOUT_ERROR: &str = "timed out waiting for spare YubiKey";

enum InteractiveDiscovery {
    Found(Vec<(String, YubiKey)>),
    NoDevice,
    OpenError {
        reader: String,
        source: YkError,
    },
}

type ReaderOpenAttempt<T> = (String, std::result::Result<T, (String, YkError)>);

enum InteractiveSelectError {
    NoDevice,
    Other(anyhow::Error),
}

type InteractiveSelectResult<T> = std::result::Result<T, InteractiveSelectError>;

fn interactive_select_error(error: impl Into<anyhow::Error>) -> InteractiveSelectError {
    InteractiveSelectError::Other(error.into())
}

fn map_select_interactive_error(err: InteractiveSelectError) -> anyhow::Error {
    match err {
        InteractiveSelectError::NoDevice => anyhow::anyhow!("no YubiKey detected"),
        InteractiveSelectError::Other(err) => err,
    }
}

/// reader open attempts を discovery 状態へ分類する。
///
/// reader が見えているのに開けない状態は、no-device ではなく最初の open error として残す。
fn classify_interactive_discovery(
    attempts: Vec<ReaderOpenAttempt<YubiKey>>,
) -> Result<InteractiveDiscovery> {
    let mut keys = Vec::new();
    let mut first_open_error = None;
    for (reader, opened) in attempts {
        match opened {
            Ok(yubikey) => keys.push((reader, yubikey)),
            Err((name, err)) if first_open_error.is_none() => {
                first_open_error = Some((name, err));
            }
            Err((_name, _err)) => {}
        }
    }

    if !keys.is_empty() {
        return Ok(InteractiveDiscovery::Found(keys));
    }

    if let Some((reader, source)) = first_open_error {
        return Ok(InteractiveDiscovery::OpenError { reader, source });
    }

    Ok(InteractiveDiscovery::NoDevice)
}

/// PC/SC reader の discovery 結果を、選択可能な device 状態へ分類する。
///
/// reader open error は保持し、権限や PC/SC 障害を no-device と誤報しない。
fn discover_interactive_yubikeys(context: &mut YkContext) -> Result<InteractiveDiscovery> {
    let attempts = context
        .iter()?
        .map(|reader| {
            let name = reader.name().into_owned();
            let opened = reader.open().map_err(|err| (name.clone(), err));
            (name, opened)
        })
        .collect::<Vec<_>>();
    classify_interactive_discovery(attempts)
}

/// 接続中の YubiKey discovery 結果を 1 本の選択結果へ変換する。
///
/// timed input が指定された場合は、複数候補の選択入力にも同じ中断と期限の契約を適用する。
fn select_interactive_yubikey_with_input(
    timed_input: Option<(Instant, &InterruptGuard)>,
    allow_no_device: bool,
) -> InteractiveSelectResult<YubiKey> {
    let mut context = YkContext::open().map_err(interactive_select_error)?;
    let discovery =
        discover_interactive_yubikeys(&mut context).map_err(interactive_select_error)?;

    match discovery {
        InteractiveDiscovery::NoDevice if allow_no_device => Err(InteractiveSelectError::NoDevice),
        InteractiveDiscovery::NoDevice => Err(interactive_select_error(anyhow::anyhow!(
            "no YubiKey detected"
        ))),
        InteractiveDiscovery::OpenError { reader, source } => {
            let err = anyhow::Error::from(source)
                .context(format!("failed to open YubiKey reader '{reader}'"));
            Err(interactive_select_error(err))
        }
        InteractiveDiscovery::Found(keys) => match keys.as_slice() {
            [_] => {
                let (_, yubikey) = keys
                    .into_iter()
                    .next()
                    .context("single selected YubiKey disappeared")
                    .map_err(interactive_select_error)?;
                Ok(yubikey)
            }
            [_, ..] => {
                let candidates = keys
                    .iter()
                    .map(|(reader, yubikey)| YubikeyCandidate {
                        reader: reader.as_str(),
                        serial: yubikey.serial().0,
                    })
                    .collect::<Vec<_>>();
                let selected = select_yubikey_candidate(&candidates, timed_input)
                    .map_err(interactive_select_error)?;
                let (_, yubikey) = keys
                    .into_iter()
                    .nth(selected)
                    .context("selected YubiKey disappeared")
                    .map_err(interactive_select_error)?;
                Ok(yubikey)
            }
            [] => Err(interactive_select_error(anyhow::anyhow!(
                "no YubiKey detected"
            ))),
        },
    }
}

/// 接続中の YubiKey から対話的に 1 本を選ぶ。
fn select_interactive_yubikey() -> Result<YubiKey> {
    select_interactive_yubikey_with_input(None, false).map_err(map_select_interactive_error)
}

/// deadline 付きの spare 待機中に、接続中の YubiKey から 1 本を選ぶ。
fn select_interactive_yubikey_until(
    deadline: Instant,
    interrupt: &InterruptGuard,
) -> InteractiveSelectResult<YubiKey> {
    select_interactive_yubikey_with_input(Some((deadline, interrupt)), true)
}

/// deadline まで対話選択可能な YubiKey を待って開く。
///
/// 未挿入状態は再試行し、reader open error は即時に呼び出し側へ返す。
fn open_interactive_device_until(
    deadline: Instant,
    interrupt: &InterruptGuard,
) -> Result<yubikey::YubikeySecretDevice> {
    loop {
        interrupt.check_interrupted()?;

        match select_interactive_yubikey_until(deadline, interrupt) {
            Ok(yk) => return Ok(yubikey::YubikeySecretDevice::from_yubikey(yk)),
            Err(InteractiveSelectError::NoDevice) => {
                let now = Instant::now();
                if now >= deadline {
                    bail!(SPARE_WAIT_TIMEOUT_ERROR);
                }
                let sleep_duration =
                    SPARE_DETECT_POLL_INTERVAL.min(deadline.saturating_duration_since(now));
                thread::sleep(sleep_duration);
            }
            Err(InteractiveSelectError::Other(err)) => return Err(err),
        }
    }
}

/// serial 指定または対話選択で実機 YubiKey device を開く。
fn open_real_device(serial: Option<u32>) -> Result<yubikey::YubikeySecretDevice> {
    let yk = if let Some(serial) = serial {
        YubiKey::open_by_serial(Serial(serial))?
    } else {
        select_interactive_yubikey()?
    };
    Ok(yubikey::YubikeySecretDevice::from_yubikey(yk))
}

/// deadline 付きで serial 指定または対話選択で実機 YubiKey device を開く。
fn open_real_device_until(
    serial: Option<u32>,
    deadline: Instant,
    interrupt: &InterruptGuard,
) -> Result<yubikey::YubikeySecretDevice> {
    interrupt.check_interrupted()?;
    let yk = if let Some(serial) = serial {
        YubiKey::open_by_serial(Serial(serial))?
    } else {
        // open_interactive_device_until returns the device directly
        return open_interactive_device_until(deadline, interrupt);
    };
    interrupt.check_interrupted()?;
    Ok(yubikey::YubikeySecretDevice::from_yubikey(yk))
}

/// spare 登録対象が primary と別 serial か確認する。
fn ensure_spare_serial(device: &yubikey::YubikeySecretDevice, primary_serial: Option<u32>) -> Result<()> {
    if Some(SecretDevice::serial(device)) == primary_serial {
        bail!("primary and spare YubiKey serial must be different");
    }
    Ok(())
}

// ── device open helpers ───────────────────────────────────────────────────────

/// backend に対応する通常操作対象 device を開く。
///
/// 非対話時の serial 必須条件は実機 adapter の error contract にする。
fn open_device(backend: &mut DeviceBackend, serial: Option<u32>) -> Result<YubikeySecretDevice> {
    match backend {
        #[cfg(feature = "secrets-test-stub")]
        DeviceBackend::TestStub { next_interactive_serial } => {
            let resolved_serial = serial.unwrap_or_else(|| {
                let s = *next_interactive_serial;
                *next_interactive_serial = SPARE_SERIAL;
                s
            });
            test_stub::TestDevice::open(resolved_serial).map(YubikeySecretDevice::TestStub)
        }
        DeviceBackend::Real => {
            #[cfg(feature = "secrets-test-stub")]
            {
                open_real_device(serial).map(YubikeySecretDevice::Real)
            }
            #[cfg(not(feature = "secrets-test-stub"))]
            {
                open_real_device(serial)
            }
        }
    }
}

/// backend に対応する spare 登録対象 device を開く。
///
/// 実機 adapter では spare 待機の interrupt policy を適用する。
fn open_spare_device(
    backend: &mut DeviceBackend,
    spare_serial: Option<u32>,
    primary_serial: Option<u32>,
    interrupt: &InterruptGuard,
) -> Result<YubikeySecretDevice> {
    match backend {
        #[cfg(feature = "secrets-test-stub")]
        DeviceBackend::TestStub { .. } => {
            let serial = spare_serial.unwrap_or(SPARE_SERIAL);
            if primary_serial == Some(serial) {
                bail!("primary and spare YubiKey serial must be different");
            }
            test_stub::TestDevice::open(serial).map(YubikeySecretDevice::TestStub)
        }
        DeviceBackend::Real => {
            let device = open_real_spare_device(spare_serial, primary_serial, interrupt)?;
            #[cfg(feature = "secrets-test-stub")]
            {
                Ok(YubikeySecretDevice::Real(device))
            }
            #[cfg(not(feature = "secrets-test-stub"))]
            {
                Ok(device)
            }
        }
    }
}

/// spare 登録対象の実機 YubiKey を開く。
///
/// `--spare-serial` があればその YubiKey を直接開く。対話実行で serial 指定がなければ、
/// まず接続済み候補から選択させる。選択結果が primary と同じ serial の場合は
/// 差し替えを促して Enter 待ちに進む。
fn open_real_spare_device(
    spare_serial: Option<u32>,
    primary_serial: Option<u32>,
    interrupt: &InterruptGuard,
) -> Result<yubikey::YubikeySecretDevice> {
    if let Some(spare_serial) = spare_serial {
        let device = open_real_device(Some(spare_serial))?;
        ensure_spare_serial(&device, primary_serial)?;
        return Ok(device);
    }

    let deadline = Instant::now() + SPARE_WAIT_TIMEOUT;
    loop {
        if Instant::now() >= deadline {
            bail!(SPARE_WAIT_TIMEOUT_ERROR);
        }
        let device = open_real_device_until(None, deadline, interrupt)?;
        if ensure_spare_serial(&device, primary_serial).is_ok() {
            return Ok(device);
        }
        wait_for_spare_replacement(deadline, interrupt)?;
    }
}

// ── RealSecretsBoundary ───────────────────────────────────────────────────────

/// 実プロセスの stdin/stdout と device backend を接続する `SecretsBoundary` 実装。
pub(super) struct RealSecretsBoundary {
    backend: DeviceBackend,
}

impl RealSecretsBoundary {
    /// 指定した backend flag で `RealSecretsBoundary` を構築する。
    pub(super) fn new(test_stub: bool) -> Result<Self> {
        let backend = DeviceBackend::from_test_flag(test_stub)?;
        Ok(Self { backend })
    }
}

impl SecretsBoundary for RealSecretsBoundary {
    type Device = YubikeySecretDevice;

    fn open_device(&mut self, serial: Option<u32>) -> Result<Self::Device> {
        open_device(&mut self.backend, serial)
    }

    fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
    ) -> Result<Self::Device> {
        let interrupt = InterruptGuard::install()
            .context("failed to install interrupt handler for spare YubiKey")?;
        open_spare_device(&mut self.backend, spare_serial, primary_serial, &interrupt)
    }

    fn require_serial(&self, serial: Option<u32>, error_message: &'static str) -> Result<()> {
        if serial.is_none() && !stdin_is_terminal() {
            bail!(error_message);
        }
        Ok(())
    }

    fn require_option(&self, enabled: bool, option_name: &'static str) -> Result<()> {
        if !enabled && !stdin_is_terminal() {
            bail!("pass {option_name} in non-interactive use");
        }
        Ok(())
    }

    fn require_stdin_pipe(&self) -> Result<()> {
        if stdin_is_terminal() {
            bail!("--stdin requires pipe or redirect input");
        }
        Ok(())
    }

    fn require_stdin_json_pipe(&self, enabled: bool) -> Result<()> {
        if enabled && stdin_is_terminal() {
            bail!("--stdin-json requires pipe or redirect input");
        }
        Ok(())
    }

    fn require_stdout_pipe(&self) -> Result<()> {
        if stdout_is_terminal() {
            bail!("refusing to write secret to terminal; redirect stdout to a file or pipe");
        }
        Ok(())
    }

    fn read_yubikey_pin_bytes(&self) -> Result<Zeroizing<Vec<u8>>> {
        read_yubikey_pin_raw()
    }

    fn read_hidden_bytes(&self, prompt_text: &str, limit: usize) -> Result<Zeroizing<Vec<u8>>> {
        read_hidden_bytes(prompt_text, limit)
    }

    fn read_visible_line_bytes(&self, prompt_text: &str, limit: usize) -> Result<Zeroizing<Vec<u8>>> {
        read_visible_line_bytes(prompt_text, limit)
    }

    fn read_stdin_bytes(&self, limit: usize) -> Result<Zeroizing<Vec<u8>>> {
        read_stdin_bytes(limit)
    }

    fn read_enrollment_json_bytes(
        &self,
        input_limit: usize,
        field_limit: usize,
    ) -> Result<EnrollmentBytes> {
        read_enrollment_json_bytes(std::io::stdin(), input_limit, field_limit)
    }

    fn write_secret_to_stdout(&self, bytes: &[u8]) -> Result<()> {
        write_secret_to_stdout(bytes)
    }

    fn write_report(&self, value: &impl serde::Serialize) -> Result<()> {
        println!("{}", serde_json::to_string_pretty(value)?);
        Ok(())
    }

    fn prompt_continue_rotation(&self) -> Result<bool> {
        prompt_yes_no(
            "Update another YubiKey? [y/N] ",
            &InterruptGuard::install()?,
        )
    }
}
