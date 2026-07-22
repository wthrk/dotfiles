//! process 標準入出力と制御端末を扱う汎用 I/O 補助。
//!
//! この module は YubiKey や use case 名を知らず、端末 raw mode、stdin/stdout の TTY 判定、
//! byte 読み取り、保護済み入力 buffer への移送だけを担当する。
//!
//! `crossterm` 0.29.0 と `filedescriptor` 0.8.3 の terminal / poll API は
//! [`external-sdk-evidence.md`](../../../../docs/secret-recovery/external-sdk-evidence.md#rust-support-crate-secret-recovery-直接利用)
//! の固定 source を根拠にする。descriptor / poll error を EOF、cancel、retryable に推測変換しない。

use std::{
    cell::Cell,
    io::{self, IsTerminal, Read, Write},
};

use anyhow::{Context, bail};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use filedescriptor::{AsRawFileDescriptor, FileDescriptor, POLLERR, POLLHUP, POLLIN, poll, pollfd};

use crate::Result;

use super::protection::{ProtectedSecret, SecretSession, buffer::ProtectedInputBuffer};

/// 制御端末優先の reader を返し、pipe 実行時も対話入力境界を維持する。
fn stdin_or_tty_reader() -> Result<FileDescriptor> {
    if io::stdin().is_terminal() {
        FileDescriptor::dup(&io::stdin()).map_err(Into::into)
    } else {
        let tty = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .context("failed to open controlling terminal")?;
        Ok(FileDescriptor::new(tty))
    }
}

/// stdin の状態に依存せず controlling terminal だけを開く。
///
/// 管理 PIN は pipeline の payload と混在させない。stdin 自体が TTY に見える場合でも、
/// controlling terminal を持たない process は PIN を受け取れず、device 操作の前に停止する。
fn controlling_tty_reader() -> Result<FileDescriptor> {
    let tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .context("failed to open controlling terminal")?;
    Ok(FileDescriptor::new(tty))
}

/// 現在の stdin が terminal に接続されているかを返す。
///
/// feature 固有の判断は行わず、adapter が対話選択を許可できる process 状態だけを公開する。
pub(crate) fn stdin_is_terminal() -> bool {
    io::stdin().is_terminal()
}

/// 制御端末から非 secret の 1 行を読み取る。
///
/// 番号選択など secret ではない入力だけに使い、secret payload は `read_hidden_line` か
/// hidden-input の protected buffer 経路へ渡す。
pub(crate) fn read_control_line(prompt: &str) -> Result<String> {
    eprint!("{prompt}");
    io::stderr().flush()?;
    let mut reader = stdin_or_tty_reader()?;
    let mut line = String::new();
    let mut byte = [0u8; 1];
    loop {
        if reader.read(&mut byte)? == 0 {
            break;
        }
        match byte[0] {
            b'\r' | b'\n' => break,
            value => line.push(char::from(value)),
        }
    }
    Ok(line)
}

/// hidden input reader の readable 状態を待つ。
fn read_hidden_byte(reader: &mut FileDescriptor, byte: &mut [u8; 1]) -> Result<usize> {
    loop {
        let mut fds = [pollfd {
            fd: reader.as_raw_file_descriptor(),
            events: POLLIN,
            revents: 0,
        }];
        let ready = poll(&mut fds, None).context("failed to poll hidden input")?;
        if ready == 0 {
            continue;
        }
        if fds[0].revents & (POLLERR | POLLHUP) != 0 && fds[0].revents & POLLIN == 0 {
            return Ok(0);
        }
        return reader.read(byte).map_err(Into::into);
    }
}

/// TTY secret prompt の hidden input に必要な byte I/O。
///
/// 実装は raw terminal を扱う `FileDescriptor`、test は in-memory fake を使う。PIN の入力
/// byte 列を端末・physical YubiKey に触れずに検証可能にするための technical seam であり、
/// command や device state の判断は持たない。
trait HiddenSecretInput {
    fn read_hidden_byte(&mut self, byte: &mut [u8; 1]) -> Result<usize>;
    /// CR を line terminator として受理したことを記録する。
    ///
    /// 次の hidden input の先頭で stream 順序どおりに到着する LF だけを捨てる。timeout 付き poll
    /// で CRLF を推測すると、遅延した LF が次 prompt の空入力になるため使わない。
    fn finish_cr_terminated_line(&mut self);
    fn write_hidden_mask(&mut self) -> Result<()>;
    fn erase_hidden_mask(&mut self) -> Result<()>;
    fn finish_hidden_line(&mut self) -> Result<()>;
}

thread_local! {
    /// controlling TTY の CRLF を prompt 境界をまたいで扱うための process-local state。
    ///
    /// raw mode では CR と LF が別 read で届くことがあり、CR の時点で LF の到着を待つと CR-only
    /// terminal の submit を停止させる。stream は順序を保つため、次の hidden read が最初に LF を
    /// 観測したときだけここで捨てれば、遅延 LF を次の secret として解釈しない。
    static DISCARD_LF_AFTER_CR: Cell<bool> = const { Cell::new(false) };
}

impl HiddenSecretInput for FileDescriptor {
    fn read_hidden_byte(&mut self, byte: &mut [u8; 1]) -> Result<usize> {
        loop {
            let read = read_hidden_byte(self, byte)?;
            if read == 0 {
                return Ok(0);
            }
            let discard_lf = DISCARD_LF_AFTER_CR.replace(false);
            if discard_lf && byte[0] == b'\n' {
                continue;
            }
            return Ok(read);
        }
    }

    fn finish_cr_terminated_line(&mut self) {
        DISCARD_LF_AFTER_CR.set(true);
    }

    fn write_hidden_mask(&mut self) -> Result<()> {
        self.write_all(b"*")?;
        self.flush()?;
        Ok(())
    }

    fn erase_hidden_mask(&mut self) -> Result<()> {
        self.write_all(b"\x08 \x08")?;
        self.flush()?;
        Ok(())
    }

    fn finish_hidden_line(&mut self) -> Result<()> {
        self.write_all(b"\n")?;
        self.flush()?;
        Ok(())
    }
}

/// secret prompt の input reader と display writer を一つの hidden-input 契約に束ねる。
///
/// `read_hidden_line` は stdin または `/dev/tty` から読み、stderr に mask を出すため、reader と
/// writer が別になる。一方 `read_hidden_tty_line` は `/dev/tty` の同じ descriptor を使う。両方を
/// 同じ raw-byte / mask contract に通すための technical adapter である。
struct HiddenPromptIo<'a, W> {
    reader: &'a mut FileDescriptor,
    writer: &'a mut W,
}

impl<W: Write> HiddenSecretInput for HiddenPromptIo<'_, W> {
    fn read_hidden_byte(&mut self, byte: &mut [u8; 1]) -> Result<usize> {
        read_hidden_byte(self.reader, byte)
    }

    fn finish_cr_terminated_line(&mut self) {
        self.reader.finish_cr_terminated_line()
    }

    fn write_hidden_mask(&mut self) -> Result<()> {
        self.writer.write_all(b"*")?;
        self.writer.flush()?;
        Ok(())
    }

    fn erase_hidden_mask(&mut self) -> Result<()> {
        self.writer.write_all(b"\x08 \x08")?;
        self.writer.flush()?;
        Ok(())
    }

    fn finish_hidden_line(&mut self) -> Result<()> {
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        Ok(())
    }
}

/// hidden controlling-TTY input を protected PIN bytes にする。
///
/// CR/LF は行終端としてのみ除去する。backspace と Ctrl-C は terminal interaction として処理するが、
/// それ以外の byte は trim、drop、Unicode/encoding 変換なしにそのまま `ProtectedSecret` へ保存する。
/// 受理した各 byte は `*` だけを TTY に出し、backspace 時は対応する mask だけを消去する。PIN
/// 本文は stdout、stderr、log、argv、environment に出さない。この raw-byte 契約は PIV VERIFY へ
/// 渡る PIN を変形せず、誤った byte 列による physical retry を増やさないためのものである。
fn read_hidden_secret_input_from(
    tty: &mut impl HiddenSecretInput,
    max_len: usize,
    too_long_message: &'static str,
) -> Result<ProtectedSecret> {
    let session = SecretSession::start()?;
    let mut input = ProtectedInputBuffer::new(max_len + 1, &session)?;
    let mut byte = [0u8; 1];
    loop {
        if tty.read_hidden_byte(&mut byte)? == 0 {
            break;
        }
        match byte[0] {
            b'\r' => {
                tty.finish_cr_terminated_line();
                tty.finish_hidden_line()?;
                break;
            }
            b'\n' => {
                tty.finish_hidden_line()?;
                break;
            }
            3 => {
                tty.finish_hidden_line()?;
                bail!("interrupted while reading hidden input");
            }
            8 | 127 => {
                if !input.as_slice().is_empty() {
                    input.pop_byte();
                    tty.erase_hidden_mask()?;
                }
            }
            value => {
                input.write_all(&[value])?;
                if input.as_slice().len() > max_len {
                    tty.finish_hidden_line()?;
                    bail!("{too_long_message}");
                }
                tty.write_hidden_mask()?;
            }
        }
    }
    input.into_protected_secret_line(&session, max_len, too_long_message)
}

const PIV_PIN_MINIMUM_BYTES: usize = 6;
const PIV_PIN_MAXIMUM_BYTES: usize = 8;

fn read_piv_pin_from(tty: &mut impl HiddenSecretInput) -> Result<ProtectedSecret> {
    let pin = read_hidden_secret_input_from(
        tty,
        PIV_PIN_MAXIMUM_BYTES,
        "YubiKey PIV PIN must contain 6 to 8 bytes",
    )?;
    if !(PIV_PIN_MINIMUM_BYTES..=PIV_PIN_MAXIMUM_BYTES).contains(&pin.len()) {
        bail!("YubiKey PIV PIN must contain 6 to 8 bytes");
    }
    Ok(pin)
}

/// 非表示入力を raw mode で読み取り、入力 bytes を保護メモリのまま返す。
///
/// backspace と Ctrl-C を process I/O 境界で吸収する。
pub(crate) fn read_hidden_line(
    prompt: &str,
    max_len: usize,
    too_long_message: &'static str,
) -> Result<ProtectedSecret> {
    let mut reader = stdin_or_tty_reader()?;
    // prompt を出す前に raw mode を設定する。canonical echo が有効なまま prompt が見えると、直後の
    // 入力 byte がこの process の mask 開始前に terminal から echo され得る。
    enable_raw_mode().context("failed to enable terminal raw mode")?;
    let _raw_mode = scopeguard::guard((), |_| {
        let _ = disable_raw_mode();
    });
    let mut stderr = io::stderr();
    stderr.write_all(prompt.as_bytes())?;
    stderr.flush()?;
    let mut prompt_io = HiddenPromptIo {
        reader: &mut reader,
        writer: &mut stderr,
    };
    read_hidden_secret_input_from(&mut prompt_io, max_len, too_long_message)
}

/// controlling TTY だけから非表示入力を読み取る。
///
/// PIV 管理 PIN のように stdin payload と絶対に混在させない値に使う。`/dev/tty` を
/// read/write で開くことに失敗した場合は prompt、raw mode、input read に進まないため、
/// caller は device mutation 前に fail-closed できる。
pub(crate) fn read_hidden_tty_line(
    prompt: &str,
    max_len: usize,
    too_long_message: &'static str,
) -> Result<ProtectedSecret> {
    let mut tty = controlling_tty_reader()?;
    // `read_hidden_line` と同様に、visible prompt より raw mode を先に設定し、prompt flush と raw-mode
    // activation の間に入力 byte が terminal echo と競合しないようにする。
    enable_raw_mode().context("failed to enable terminal raw mode")?;
    let _raw_mode = scopeguard::guard((), |_| {
        let _ = disable_raw_mode();
    });
    tty.write_all(prompt.as_bytes())?;
    tty.flush()?;
    read_hidden_secret_input_from(&mut tty, max_len, too_long_message)
}

/// controlling TTY から PIV PIN の指定 byte 範囲だけを受け取る。
///
/// [Yubico PIV VERIFY specification](https://docs.yubico.com/yesdk/users-manual/application-piv/apdu/verify.html#verify-pin)
/// が定める PIN の 6--8 byte 制約を device/session 開始前に適用する。EOF、空入力、範囲外入力は
/// VERIFY へ渡さない。
pub(crate) fn read_hidden_tty_piv_pin() -> Result<ProtectedSecret> {
    let mut tty = controlling_tty_reader()?;
    // `read_hidden_line` と同様に、visible prompt より raw mode を先に設定し、prompt flush と raw-mode
    // activation の間に入力 byte が terminal echo と競合しないようにする。
    enable_raw_mode().context("failed to enable terminal raw mode")?;
    let _raw_mode = scopeguard::guard((), |_| {
        let _ = disable_raw_mode();
    });
    tty.write_all(b"YubiKey PIV PIN: ")?;
    tty.flush()?;
    read_piv_pin_from(&mut tty)
}

/// 制御端末から非秘匿の 1 行を可視入力（通常の echo つき cooked 入力）で読み取る。
///
/// `password-store-remote` の clone URL のように秘密情報でない値の対話入力に使う。raw mode や保護 buffer は
/// 使わず、利用者が打った文字は端末が echo する。返値は末尾改行を除いた平文 `String` で、保護義務を負わない。
pub(crate) fn read_visible_plain_line(
    prompt: &str,
    max_len: usize,
    too_long_message: &'static str,
) -> Result<String> {
    eprint!("{prompt}");
    io::stderr().flush()?;
    let mut reader = stdin_or_tty_reader()?;
    let line = read_plain_line_from(&mut reader, max_len, too_long_message)?;
    // prompt 行を改行で閉じ、後続の stdout 出力が prompt 文言と同じ行に連結しないようにする。
    eprintln!();
    Ok(line)
}

/// stdin（pipe / redirect）から非秘匿の 1 行を読み取り、末尾改行を除いた平文 `String` を返す。
///
/// stdin が terminal の場合は pipe 入力を要求して停止する。`password-store-remote` の clone URL のように
/// 秘密情報でない値の非対話入力に使い、保護 buffer は使わない。
pub(crate) fn read_stdin_plain_line(
    max_len: usize,
    too_long_message: &'static str,
) -> Result<String> {
    if io::stdin().is_terminal() {
        bail!("stdin URL input requires pipe or redirect input");
    }
    read_plain_line_from(&mut io::stdin(), max_len, too_long_message)
}

/// reader から改行までの 1 行を平文 `String` として読み取る共通実装。
///
/// `max_len` を超えた時点で `too_long_message` を返して停止する。行末は LF（`\n`）・CR（`\r`）・CRLF
/// （`\r\n`）のいずれも 1 改行として扱う。`\r` を読んだら最初の行をそこで終端し、続く 1 byte が `\n` なら
/// CRLF として一緒に消費して reader に余分な `\n` を残さない（`\r` の後が `\n` 以外の byte の場合、本 primitive は
/// 1 行のみを読むためその 1 byte は読み捨てる）。行末文字自体は返値に含めない。上限超過文言は feature 固有語彙の
/// ため caller（adapter）から受け取り、support には焼き込まない。secret ではない値だけに使う。
fn read_plain_line_from(
    reader: &mut impl Read,
    max_len: usize,
    too_long_message: &'static str,
) -> Result<String> {
    let mut line = String::new();
    let mut byte = [0u8; 1];
    loop {
        if reader.read(&mut byte)? == 0 {
            break;
        }
        match byte[0] {
            b'\n' => break,
            b'\r' => {
                // CR で行を終端する。CRLF の場合は直後の `\n` を 1 byte 先読みして一緒に消費し、reader へ
                // 余分な `\n` を残さない。`\r` の後が `\n` 以外なら本 primitive は 1 行のみ読むため読み捨てる。
                let mut next = [0u8; 1];
                let _ = reader.read(&mut next)?;
                break;
            }
            value => {
                line.push(char::from(value));
                if line.len() > max_len {
                    bail!("{too_long_message}");
                }
            }
        }
    }
    Ok(line)
}

/// stdin 1 行を読み取り、末尾改行を除いた保護済み secret を返す。
pub(crate) fn read_stdin_line(
    max_len: usize,
    too_long_message: &'static str,
) -> Result<ProtectedSecret> {
    if io::stdin().is_terminal() {
        bail!("stdin secret input requires pipe or redirect input");
    }
    let session = SecretSession::start()?;
    let input = ProtectedInputBuffer::read_line_from(io::stdin(), max_len, &session)?;
    input.into_protected_secret_line(&session, max_len, too_long_message)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, io::Cursor};

    use crate::{Result, support::protection::yubikey_piv::verify_pin_with};

    use super::{HiddenSecretInput, read_hidden_secret_input_from, read_piv_pin_from};

    struct FakeControllingTty {
        input: Cursor<Vec<u8>>,
        output: Vec<u8>,
        finished: bool,
        discard_lf_after_cr: bool,
    }

    impl FakeControllingTty {
        fn new(input: &[u8]) -> Self {
            Self {
                input: Cursor::new(input.to_vec()),
                output: Vec::new(),
                finished: false,
                discard_lf_after_cr: false,
            }
        }
    }

    impl HiddenSecretInput for FakeControllingTty {
        fn read_hidden_byte(&mut self, byte: &mut [u8; 1]) -> Result<usize> {
            loop {
                let read = std::io::Read::read(&mut self.input, byte)?;
                if read == 0 {
                    return Ok(0);
                }
                let discard_lf = std::mem::take(&mut self.discard_lf_after_cr);
                if discard_lf && byte[0] == b'\n' {
                    continue;
                }
                return Ok(read);
            }
        }

        fn finish_cr_terminated_line(&mut self) {
            self.discard_lf_after_cr = true;
        }

        fn write_hidden_mask(&mut self) -> Result<()> {
            self.output.push(b'*');
            Ok(())
        }

        fn erase_hidden_mask(&mut self) -> Result<()> {
            self.output.extend_from_slice(b"\x08 \x08");
            Ok(())
        }

        fn finish_hidden_line(&mut self) -> Result<()> {
            self.finished = true;
            self.output.push(b'\n');
            Ok(())
        }
    }

    #[test]
    fn controlling_tty_pin_bytes_reach_verify_unmodified_except_line_terminator() -> Result<()> {
        // Include whitespace and non-UTF-8 bytes to prove the PIN path neither trims nor
        // performs text/encoding conversion. The fake replaces both the TTY and YubiKey.
        let expected = b" 12\x80\xffA";
        let mut tty = FakeControllingTty::new(b" 12\x80\xffA\r");
        let pin = read_hidden_secret_input_from(&mut tty, 64, "too large")?;
        let observed = RefCell::new(Vec::new());

        verify_pin_with(&pin, |bytes| {
            observed.replace(bytes.to_vec());
            Ok(())
        })?;

        assert!(tty.finished);
        assert_eq!(observed.into_inner(), expected);
        Ok(())
    }

    #[test]
    fn controlling_tty_lf_is_only_a_line_terminator() -> Result<()> {
        let mut tty = FakeControllingTty::new(b"12 34\n");
        let pin = read_hidden_secret_input_from(&mut tty, 64, "too large")?;

        assert!(tty.finished);
        assert_eq!(pin.to_test_bytes(), b"12 34");
        Ok(())
    }

    #[test]
    fn delayed_crlf_is_not_interpreted_as_the_next_hidden_prompt_input() -> Result<()> {
        let mut tty = FakeControllingTty::new(b"123456\r\nnext-token");
        let pin = read_piv_pin_from(&mut tty)?;
        let token = read_hidden_secret_input_from(&mut tty, 64, "too large")?;

        assert!(tty.finished);
        assert_eq!(pin.to_test_bytes(), b"123456");
        assert_eq!(token.to_test_bytes(), b"next-token");
        let position = tty.input.position() as usize;
        assert_eq!(&tty.input.get_ref()[position..], b"");
        Ok(())
    }

    #[test]
    fn invalid_piv_pin_never_reaches_verify_callback() {
        for input in [b"12345\n".as_slice(), b"", b"123456789\n"] {
            let mut tty = FakeControllingTty::new(input);
            let verify_calls = RefCell::new(0usize);
            let result = read_piv_pin_from(&mut tty).and_then(|pin| {
                verify_pin_with(&pin, |_| {
                    *verify_calls.borrow_mut() += 1;
                    Ok(())
                })
            });

            assert!(result.is_err(), "invalid PIN input must fail closed");
            assert_eq!(*verify_calls.borrow(), 0, "VERIFY must not be called");
        }
    }

    #[test]
    fn eight_byte_piv_pin_reaches_verify_once_without_transformation() -> Result<()> {
        let mut tty = FakeControllingTty::new(b"12345678\n");
        let pin = read_piv_pin_from(&mut tty)?;
        let calls = RefCell::new(0_usize);
        verify_pin_with(&pin, |bytes| {
            *calls.borrow_mut() += 1;
            assert_eq!(bytes, b"12345678");
            Ok(())
        })?;

        assert_eq!(*calls.borrow(), 1);
        Ok(())
    }

    #[test]
    fn controlling_tty_masks_input_and_keeps_stdin_token_separate() -> Result<()> {
        let stdin_token = b"stdin-token-must-not-become-a-pin";
        let expected_pin = b"a b\x80C";
        let mut tty = FakeControllingTty::new(b"a b\x80\xff\x08C\r");
        let pin = read_hidden_secret_input_from(&mut tty, 64, "too large")?;
        let observed = RefCell::new(Vec::new());

        verify_pin_with(&pin, |bytes| {
            observed.replace(bytes.to_vec());
            Ok(())
        })?;

        assert_eq!(observed.into_inner(), expected_pin);
        assert_ne!(expected_pin.as_slice(), stdin_token.as_slice());
        assert_eq!(tty.output, b"*****\x08 \x08*\n");
        assert!(
            !tty.output
                .windows(expected_pin.len())
                .any(|window| window == expected_pin),
            "TTY mask output must never contain PIN bytes"
        );
        assert!(
            !tty.output
                .windows(stdin_token.len())
                .any(|window| window == stdin_token),
            "TTY mask output must never contain stdin token bytes"
        );
        Ok(())
    }
}

/// stdin 全体を保護バッファへ読み取り、末尾改行を保持したまま追加の平文複製なしで返す。
pub(crate) fn read_stdin_all(
    max_len: usize,
    too_long_message: &'static str,
) -> Result<ProtectedSecret> {
    if io::stdin().is_terminal() {
        bail!("stdin document input requires pipe or redirect input");
    }
    let session = SecretSession::start()?;
    let input = ProtectedInputBuffer::read_from(io::stdin(), max_len, too_long_message, &session)?;
    input.into_protected_secret(&session, max_len, too_long_message)
}

#[cfg(test)]
mod plain_line_tests {
    //! 非秘匿 1 行読み取りの行末処理（LF / CR / CRLF）が doc どおり 1 改行として扱われ、CRLF 入力で
    //! reader へ末尾 `\r` も余分な `\n` も残さないことを byte slice reader で検証する。

    use super::read_plain_line_from;

    const TOO_LONG: &str = "input too long";

    #[test]
    fn reads_line_terminated_by_lf() {
        let mut reader: &[u8] = b"https://example.test/repo.git\nnext";
        let line = read_plain_line_from(&mut reader, 1024, TOO_LONG).expect("read line");
        assert_eq!(line, "https://example.test/repo.git");
        // LF は消費され、後続データは reader に残る。
        assert_eq!(reader, b"next");
    }

    #[test]
    fn reads_line_terminated_by_lone_cr() {
        let mut reader: &[u8] = b"value\rnext";
        let line = read_plain_line_from(&mut reader, 1024, TOO_LONG).expect("read line");
        assert_eq!(line, "value");
    }

    #[test]
    fn crlf_is_consumed_as_single_newline_without_residual_lf() {
        let mut reader: &[u8] = b"value\r\nnext";
        let line = read_plain_line_from(&mut reader, 1024, TOO_LONG).expect("read line");
        assert_eq!(line, "value");
        // CRLF の `\n` まで消費し、reader に余分な `\n` を残さない。
        assert_eq!(reader, b"next");
    }

    #[test]
    fn line_without_terminator_returns_until_eof() {
        let mut reader: &[u8] = b"value";
        let line = read_plain_line_from(&mut reader, 1024, TOO_LONG).expect("read line");
        assert_eq!(line, "value");
    }

    #[test]
    fn exceeding_max_len_fails() {
        let mut reader: &[u8] = b"0123456789\n";
        let result = read_plain_line_from(&mut reader, 4, TOO_LONG);
        assert!(result.is_err(), "over-length input must fail");
    }
}
