//! process 標準入出力と制御端末を扱う汎用 I/O 補助。
//!
//! この module は YubiKey や use case 名を知らず、端末 raw mode、stdin/stdout の TTY 判定、
//! byte 読み取り、保護済み入力 buffer への移送だけを担当する。

use std::io::{self, IsTerminal, Read, Write};

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

/// 現在の stdin が terminal に接続されているかを返す。
///
/// feature 固有の判断は行わず、adapter が端末入力を許可できる process 状態だけを公開する。
pub(crate) fn stdin_is_terminal() -> bool {
    io::stdin().is_terminal()
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

/// 非表示入力を raw mode で読み取り、入力 bytes を保護メモリのまま返す。
///
/// backspace と Ctrl-C を process I/O 境界で吸収する。
pub(crate) fn read_hidden_line(
    prompt: &str,
    max_len: usize,
    too_long_message: &'static str,
) -> Result<ProtectedSecret> {
    let session = SecretSession::start()?;
    eprint!("{prompt}");
    io::stderr().flush()?;
    let mut reader = stdin_or_tty_reader()?;
    enable_raw_mode().context("failed to enable terminal raw mode")?;
    let _raw_mode = scopeguard::guard((), |_| {
        let _ = disable_raw_mode();
    });
    let mut input = ProtectedInputBuffer::new(max_len + 1, &session)?;
    let mut byte = [0u8; 1];
    loop {
        if read_hidden_byte(&mut reader, &mut byte)? == 0 {
            break;
        }
        match byte[0] {
            b'\r' | b'\n' => {
                eprintln!();
                break;
            }
            3 => bail!("interrupted while reading hidden input"),
            8 | 127 => input.pop_byte(),
            value => {
                input.write_all(&[value])?;
                if input.as_slice().len() > max_len {
                    bail!("{too_long_message}");
                }
            }
        }
    }
    input.into_protected_secret_line(&session, max_len, too_long_message)
}

/// visible 入力を保護バッファへ直接取り込み、平文コピーを残さない。
pub(crate) fn read_visible_line(
    prompt: &str,
    max_len: usize,
    too_long_message: &'static str,
) -> Result<ProtectedSecret> {
    let session = SecretSession::start()?;
    eprint!("{prompt}");
    io::stderr().flush()?;
    let mut reader = stdin_or_tty_reader()?;
    let input = ProtectedInputBuffer::read_line_from(&mut reader, max_len, &session)?;
    input.into_protected_secret_line(&session, max_len, too_long_message)
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
/// stdin が terminal の場合は pipe 入力を要求して停止する。YubiKey OTP のように secret ではない値の
/// 非対話入力に使い、保護 buffer は使わない。
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

/// terminal 直書きを拒否し、caller supplied secret writer を stdout redirect 境界で実行する。
pub(crate) fn write_secret_stdout_with(
    write_secret: impl FnOnce(&mut std::io::StdoutLock<'_>) -> Result<()>,
) -> Result<()> {
    if io::stdout().is_terminal() {
        bail!("refusing to write secret to terminal; redirect stdout to a file or pipe");
    }
    write_secret(&mut io::stdout().lock())
}

#[cfg(test)]
mod tests {
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
