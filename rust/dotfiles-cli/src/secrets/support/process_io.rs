//! process 標準入出力と制御端末を扱う汎用 I/O 補助。
//!
//! この module は YubiKey や use case 名を知らず、端末 raw mode、stdin/stdout の TTY 判定、
//! byte stream 読み取り、保護済み入力 buffer への移送だけを担当する。

#[cfg(any(test, not(feature = "secrets-internal-test-stub")))]
use std::io::Read;
#[cfg(not(feature = "secrets-internal-test-stub"))]
use std::io::Write;
use std::io::{self, IsTerminal};

#[cfg(not(feature = "secrets-internal-test-stub"))]
use anyhow::Context;
use anyhow::bail;
#[cfg(not(feature = "secrets-internal-test-stub"))]
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
#[cfg(not(feature = "secrets-internal-test-stub"))]
use filedescriptor::{AsRawFileDescriptor, FileDescriptor, POLLERR, POLLHUP, POLLIN, poll, pollfd};

use crate::Result;

#[cfg(not(feature = "secrets-internal-test-stub"))]
use super::protection::{
    ProtectedSecret, SecretSession, TransientSecretBytes, buffer::ProtectedInputBuffer,
};

#[cfg(not(feature = "secrets-internal-test-stub"))]
const BRACKETED_PASTE_MARKER_LEN: usize = 6;
#[cfg(not(feature = "secrets-internal-test-stub"))]
const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";
#[cfg(not(feature = "secrets-internal-test-stub"))]
const BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";
#[cfg(not(feature = "secrets-internal-test-stub"))]
const HIDDEN_INPUT_CHUNK_LEN: usize = 1024;

/// 制御端末優先の reader を返し、pipe 実行時も対話入力境界を維持する。
#[cfg(not(feature = "secrets-internal-test-stub"))]
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

/// hidden input reader の readable 状態を待つ。
#[cfg(not(feature = "secrets-internal-test-stub"))]
fn read_hidden_chunk(reader: &mut FileDescriptor, chunk: &mut [u8]) -> Result<usize> {
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
        return reader.read(chunk).map_err(Into::into);
    }
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HiddenInputStatus {
    Continue,
    Complete,
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
/// hidden/no-echo 入力で bracketed paste marker を除去する filter。
///
/// ESC 始まりの pending bytes は secret 本文断片になり得るため `TransientSecretBytes` に一時保持し、
/// この型は marker 判定と zeroize/clear 境界だけを担う。caller は hidden/no-echo 入力から得た bytes に限って渡す。
struct BracketedPasteFilter {
    pending: TransientSecretBytes<BRACKETED_PASTE_MARKER_LEN>,
    pending_len: usize,
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
impl BracketedPasteFilter {
    /// bracketed paste marker 判定用の一時 pending buffer を初期化する。
    ///
    /// pending は ESC から始まった入力断片を一時的に保持するため、marker ではない secret 本文が
    /// 入る場合がある。zeroize は `TransientSecretBytes` に委譲し、この filter は marker
    /// 判定と clear タイミングだけを担当する。
    fn new() -> Self {
        Self {
            pending: TransientSecretBytes::new(),
            pending_len: 0,
        }
    }

    /// 1 byte を marker 判定に通し、secret 本文だけを protected input buffer へ移す。
    ///
    /// caller は hidden/no-echo 入力から得た byte だけを渡す。pending に保持した bytes は、
    /// marker と確定した時、不一致で本文へ flush した時、または flush が失敗した時に clear する。
    fn push(
        &mut self,
        byte: u8,
        input: &mut ProtectedInputBuffer,
        max_len: usize,
        too_long_message: &'static str,
    ) -> Result<HiddenInputStatus> {
        if self.pending_len == 0 {
            if byte == BRACKETED_PASTE_START[0] {
                self.pending.set(0, byte);
                self.pending_len = 1;
                return Ok(HiddenInputStatus::Continue);
            }
            return push_hidden_input_byte(input, byte, max_len, too_long_message);
        }

        self.pending.set(self.pending_len, byte);
        self.pending_len += 1;
        let pending = self.pending.prefix(self.pending_len);
        if pending == BRACKETED_PASTE_START || pending == BRACKETED_PASTE_END {
            self.clear_pending();
            return Ok(HiddenInputStatus::Continue);
        }
        if BRACKETED_PASTE_START.starts_with(pending) || BRACKETED_PASTE_END.starts_with(pending) {
            return Ok(HiddenInputStatus::Continue);
        }

        let status = flush_hidden_input_pending(pending, input, max_len, too_long_message);
        self.clear_pending();
        status
    }

    /// EOF 時に marker 未確定の pending bytes を本文として処理する。
    ///
    /// 完了・継続・エラーのいずれでも pending を clear し、未確定の secret 断片を filter 内に残さない。
    fn finish(
        &mut self,
        input: &mut ProtectedInputBuffer,
        max_len: usize,
        too_long_message: &'static str,
    ) -> Result<HiddenInputStatus> {
        let status = flush_hidden_input_pending(
            self.pending.prefix(self.pending_len),
            input,
            max_len,
            too_long_message,
        );
        self.clear_pending();
        status
    }

    fn clear_pending(&mut self) {
        self.pending_len = 0;
        self.pending.clear();
    }
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
impl Drop for BracketedPasteFilter {
    fn drop(&mut self) {
        self.clear_pending();
    }
}

/// marker ではない pending bytes を hidden input として protected buffer へ戻す。
///
/// caller はこの関数の成否にかかわらず、元の pending owner を clear する責務を持つ。
#[cfg(not(feature = "secrets-internal-test-stub"))]
fn flush_hidden_input_pending(
    pending: &[u8],
    input: &mut ProtectedInputBuffer,
    max_len: usize,
    too_long_message: &'static str,
) -> Result<HiddenInputStatus> {
    for byte in pending {
        if push_hidden_input_byte(input, *byte, max_len, too_long_message)?
            == HiddenInputStatus::Complete
        {
            return Ok(HiddenInputStatus::Complete);
        }
    }
    Ok(HiddenInputStatus::Continue)
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
/// hidden/no-echo 入力の制御 bytes を処理し、本文 byte だけを protected buffer へ書く。
///
/// 上限超過や書き込み失敗時も secret 本文は error 文字列へ含めない。caller は未処理の一時
/// chunk/pending を protection wrapper 経由で破棄する。
fn push_hidden_input_byte(
    input: &mut ProtectedInputBuffer,
    byte: u8,
    max_len: usize,
    too_long_message: &'static str,
) -> Result<HiddenInputStatus> {
    match byte {
        b'\r' | b'\n' => Ok(HiddenInputStatus::Complete),
        3 => bail!("interrupted while reading hidden input"),
        8 | 127 => {
            input.pop_byte();
            Ok(HiddenInputStatus::Continue)
        }
        value => {
            input.write_all(&[value])?;
            if input.as_slice().len() > max_len {
                bail!("{too_long_message}");
            }
            Ok(HiddenInputStatus::Continue)
        }
    }
}

/// reader から hidden line を読み、terminal paste marker を secret buffer へ混入させない。
#[cfg(all(test, not(feature = "secrets-internal-test-stub")))]
fn read_hidden_line_from(
    reader: &mut impl Read,
    max_len: usize,
    too_long_message: &'static str,
    session: &SecretSession,
) -> Result<ProtectedSecret> {
    let mut input = ProtectedInputBuffer::new(max_len + 1, session)?;
    let mut chunk = TransientSecretBytes::<HIDDEN_INPUT_CHUNK_LEN>::new();
    let mut paste_filter = BracketedPasteFilter::new();
    loop {
        let read = reader.read(chunk.as_mut_slice())?;
        if read == 0 {
            if paste_filter.finish(&mut input, max_len, too_long_message)?
                == HiddenInputStatus::Complete
            {
                break;
            }
            break;
        }
        for byte in &chunk.as_slice()[..read] {
            if paste_filter.push(*byte, &mut input, max_len, too_long_message)?
                == HiddenInputStatus::Complete
            {
                return input.into_protected_secret_line(session, max_len, too_long_message);
            }
        }
    }
    input.into_protected_secret_line(session, max_len, too_long_message)
}

/// 非表示入力を raw mode で読み取り、入力 bytes を保護メモリのまま返す。
///
/// backspace と Ctrl-C を process I/O 境界で吸収する。paste が terminal から bracketed paste
/// marker つきで届いた場合、marker は制御列として扱い secret buffer へ入れない。
#[cfg(not(feature = "secrets-internal-test-stub"))]
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
    let mut chunk = TransientSecretBytes::<HIDDEN_INPUT_CHUNK_LEN>::new();
    let mut paste_filter = BracketedPasteFilter::new();
    loop {
        let read = read_hidden_chunk(&mut reader, chunk.as_mut_slice())?;
        if read == 0 {
            break;
        }
        for byte in &chunk.as_slice()[..read] {
            if paste_filter.push(*byte, &mut input, max_len, too_long_message)?
                == HiddenInputStatus::Complete
            {
                eprintln!();
                return input.into_protected_secret_line(&session, max_len, too_long_message);
            }
        }
    }
    if paste_filter.finish(&mut input, max_len, too_long_message)? == HiddenInputStatus::Complete {
        eprintln!();
    }
    input.into_protected_secret_line(&session, max_len, too_long_message)
}

/// 制御端末から非秘匿の 1 行を可視入力（通常の echo つき cooked 入力）で読み取る。
///
/// `password-store-remote` の clone URL のように秘密情報でない値の対話入力に使う。raw mode や保護 buffer は
/// 使わず、利用者が打った文字は端末が echo する。返値は末尾改行を除いた平文 `String` で、保護義務を負わない。
#[cfg(not(feature = "secrets-internal-test-stub"))]
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

/// reader から改行までの 1 行を平文 `String` として読み取る共通実装。
///
/// `max_len` を超えた時点で `too_long_message` を返して停止する。行末は LF（`\n`）・CR（`\r`）・CRLF
/// （`\r\n`）のいずれも 1 改行として扱う。`\r` を読んだら最初の行をそこで終端し、続く 1 byte が `\n` なら
/// CRLF として一緒に消費して reader に余分な `\n` を残さない（`\r` の後が `\n` 以外の byte の場合、本 primitive は
/// 1 行のみを読むためその 1 byte は読み捨てる）。行末文字自体は返値に含めない。上限超過文言は feature 固有語彙の
/// ため caller（adapter）から受け取り、support には焼き込まない。secret ではない値だけに使う。
#[cfg(any(test, not(feature = "secrets-internal-test-stub")))]
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
    //! 非秘匿 1 行読み取りの行末処理と、秘匿入力 paste 経路を byte reader で検証する。

    #[cfg(not(feature = "secrets-internal-test-stub"))]
    use std::io::{self, IsTerminal, Read, Write};
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    use std::time::{Duration, Instant};

    #[cfg(not(feature = "secrets-internal-test-stub"))]
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    use sha2::{Digest, Sha256};

    #[cfg(not(feature = "secrets-internal-test-stub"))]
    use crate::{Result, secrets::support::protection::SecretSession};

    use super::read_plain_line_from;
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    use super::{
        BRACKETED_PASTE_END, BRACKETED_PASTE_START, read_hidden_line, read_hidden_line_from,
        write_secret_stdout_with,
    };

    const TOO_LONG: &str = "input too long";
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    const PTY_CHILD_ENV: &str = "DOTFILES_PROCESS_IO_PTY_HIDDEN_CHILD";

    #[cfg(not(feature = "secrets-internal-test-stub"))]
    struct ChunkReader<'a> {
        chunks: &'a [&'a [u8]],
        index: usize,
    }

    #[cfg(not(feature = "secrets-internal-test-stub"))]
    impl<'a> ChunkReader<'a> {
        fn new(chunks: &'a [&'a [u8]]) -> Self {
            Self { chunks, index: 0 }
        }
    }

    #[cfg(not(feature = "secrets-internal-test-stub"))]
    impl Read for ChunkReader<'_> {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            let Some(chunk) = self.chunks.get(self.index) else {
                return Ok(0);
            };
            self.index += 1;
            let len = chunk.len().min(output.len());
            output[..len].copy_from_slice(&chunk[..len]);
            Ok(len)
        }
    }

    #[cfg(not(feature = "secrets-internal-test-stub"))]
    fn assert_secret_bytes_eq(actual: &[u8], expected: &[u8], label: &str) {
        let actual_digest: [u8; 32] = Sha256::digest(actual).into();
        let expected_digest: [u8; 32] = Sha256::digest(expected).into();

        assert_eq!(actual.len(), expected.len(), "{label} length mismatch");
        assert_eq!(actual_digest, expected_digest, "{label} digest mismatch");
    }

    #[cfg(not(feature = "secrets-internal-test-stub"))]
    fn assert_hidden_too_long(
        result: Result<crate::secrets::support::protection::ProtectedSecret>,
    ) {
        let error = match result {
            Ok(_) => panic!("over-limit hidden input must fail"),
            Err(error) => error,
        };
        let text = error.to_string();
        assert!(
            text.contains(TOO_LONG),
            "hidden input over-limit error must use fixed caller message"
        );
        assert!(
            !text.contains("pasted-client-secret"),
            "hidden input over-limit error must not include pasted secret fixture"
        );
        assert!(
            !text.contains("secret-fragment"),
            "hidden input over-limit error must not include escape-prefixed secret fixture"
        );
    }

    #[cfg(not(feature = "secrets-internal-test-stub"))]
    fn read_until_prompt(reader: &mut dyn Read, prompt: &str) -> Result<String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = Vec::new();
        let mut byte = [0u8; 1];
        while Instant::now() < deadline {
            if reader.read(&mut byte)? == 0 {
                continue;
            }
            output.push(byte[0]);
            let text = String::from_utf8_lossy(&output);
            if text.contains(prompt) {
                return Ok(text.into_owned());
            }
        }
        anyhow::bail!("timed out waiting for hidden prompt")
    }

    /// LF 終端の入力では行本体だけを返し、後続 bytes を reader に残す。
    #[test]
    fn reads_line_terminated_by_lf() {
        let mut reader: &[u8] = b"https://example.test/repo.git\nnext";
        let line = read_plain_line_from(&mut reader, 1024, TOO_LONG).expect("read line");
        assert_eq!(line, "https://example.test/repo.git");
        // LF は消費され、後続データは reader に残る。
        assert_eq!(reader, b"next");
    }

    /// 単独 CR 終端の入力では CR を行末として扱い、返値に含めない。
    #[test]
    fn reads_line_terminated_by_lone_cr() {
        let mut reader: &[u8] = b"value\rnext";
        let line = read_plain_line_from(&mut reader, 1024, TOO_LONG).expect("read line");
        assert_eq!(line, "value");
    }

    /// CRLF 終端の入力では CRLF 全体を 1 改行として消費し、余分な LF を残さない。
    #[test]
    fn crlf_is_consumed_as_single_newline_without_residual_lf() {
        let mut reader: &[u8] = b"value\r\nnext";
        let line = read_plain_line_from(&mut reader, 1024, TOO_LONG).expect("read line");
        assert_eq!(line, "value");
        // CRLF の `\n` まで消費し、reader に余分な `\n` を残さない。
        assert_eq!(reader, b"next");
    }

    /// 終端なしの入力では EOF までの内容を 1 行として返す。
    #[test]
    fn line_without_terminator_returns_until_eof() {
        let mut reader: &[u8] = b"value";
        let line = read_plain_line_from(&mut reader, 1024, TOO_LONG).expect("read line");
        assert_eq!(line, "value");
    }

    /// 最大長を超えた入力は caller supplied message で失敗する。
    #[test]
    fn exceeding_max_len_fails() {
        let mut reader: &[u8] = b"0123456789\n";
        let result = read_plain_line_from(&mut reader, 4, TOO_LONG);
        assert!(result.is_err(), "over-length input must fail");
    }

    /// paste 相当で複数 byte が 1 回の read で届いても、改行までの全 bytes を秘匿値として受け付ける。
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    #[test]
    fn hidden_line_accepts_pasted_bytes_delivered_in_one_read() -> Result<()> {
        let session = SecretSession::start()?;
        let mut reader: &[u8] = b"pasted-client-secret\n";
        let secret = read_hidden_line_from(&mut reader, 1024, TOO_LONG, &session)?;

        assert_secret_bytes_eq(
            &secret.to_test_bytes(),
            b"pasted-client-secret",
            "pasted hidden input",
        );
        Ok(())
    }

    /// terminal が bracketed paste marker を付けても、marker を secret buffer へ混入させない。
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    #[test]
    fn hidden_line_strips_bracketed_paste_markers() -> Result<()> {
        let session = SecretSession::start()?;
        let mut input = Vec::new();
        input.extend_from_slice(BRACKETED_PASTE_START);
        input.extend_from_slice(b"pasted-client-secret");
        input.extend_from_slice(BRACKETED_PASTE_END);
        input.push(b'\n');
        let mut reader = input.as_slice();
        let secret = read_hidden_line_from(&mut reader, 1024, TOO_LONG, &session)?;

        assert_secret_bytes_eq(
            &secret.to_test_bytes(),
            b"pasted-client-secret",
            "bracketed pasted hidden input",
        );
        Ok(())
    }

    /// bracketed paste marker が read 境界で分割されても、marker を secret buffer へ混入させない。
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    #[test]
    fn hidden_line_strips_bracketed_paste_markers_split_across_reads() -> Result<()> {
        let session = SecretSession::start()?;
        let chunks: &[&[u8]] = &[b"\x1b[2", b"00~pasted-", b"client-secret\x1b[20", b"1~\n"];
        let mut reader = ChunkReader::new(chunks);
        let secret = read_hidden_line_from(&mut reader, 1024, TOO_LONG, &session)?;

        assert_secret_bytes_eq(
            &secret.to_test_bytes(),
            b"pasted-client-secret",
            "split bracketed pasted hidden input",
        );
        Ok(())
    }

    /// ESC から始まっても bracketed paste marker でなければ、本文として保持する。
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    #[test]
    fn hidden_line_preserves_escape_prefixed_non_marker_input() -> Result<()> {
        let session = SecretSession::start()?;
        let mut reader: &[u8] = b"\x1bsecret-fragment\n";
        let secret = read_hidden_line_from(&mut reader, 1024, TOO_LONG, &session)?;

        assert_secret_bytes_eq(
            &secret.to_test_bytes(),
            b"\x1bsecret-fragment",
            "escape-prefixed hidden input",
        );
        Ok(())
    }

    /// paste 相当で複数 byte が 1 回の read で届く hidden 入力でも、本文上限超過は固定 error で失敗する。
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    #[test]
    fn hidden_line_rejects_over_limit_pasted_bytes_delivered_in_one_read() -> Result<()> {
        let session = SecretSession::start()?;
        let mut reader: &[u8] = b"pasted-client-secret\n";

        assert_hidden_too_long(read_hidden_line_from(&mut reader, 4, TOO_LONG, &session));
        Ok(())
    }

    /// marker に見えかけて不一致になった pending 本文で上限超過しても、固定 error だけを返す。
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    #[test]
    fn hidden_line_rejects_over_limit_escape_prefixed_non_marker_input() -> Result<()> {
        let session = SecretSession::start()?;
        let mut reader: &[u8] = b"\x1bsecret-fragment\n";

        assert_hidden_too_long(read_hidden_line_from(&mut reader, 4, TOO_LONG, &session));
        Ok(())
    }

    /// bracketed paste marker 付き hidden 入力では、marker を除いた本文に max_len を適用する。
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    #[test]
    fn hidden_line_rejects_over_limit_bracketed_paste_body() -> Result<()> {
        let session = SecretSession::start()?;
        let mut input = Vec::new();
        input.extend_from_slice(BRACKETED_PASTE_START);
        input.extend_from_slice(b"pasted-client-secret");
        input.extend_from_slice(BRACKETED_PASTE_END);
        input.push(b'\n');
        let mut reader = input.as_slice();

        assert_hidden_too_long(read_hidden_line_from(&mut reader, 4, TOO_LONG, &session));
        Ok(())
    }

    /// PTY 上の実 hidden prompt に paste 相当の byte 列を一括投入し、TTY/raw/poll/read 経路を通して検証する。
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    #[test]
    fn pty_hidden_prompt_accepts_pasted_bracketed_input() -> Result<()> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut command = CommandBuilder::new(std::env::current_exe()?);
        command.args([
            "--exact",
            "secrets::support::process_io::tests::pty_hidden_child_reads_pasted_input",
            "--nocapture",
        ]);
        command.env(PTY_CHILD_ENV, "1");
        let mut child = pair.slave.spawn_command(command)?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let mut writer = pair.master.take_writer()?;
        let prompt_output = read_until_prompt(&mut *reader, "hidden-test: ")?;
        assert!(
            !prompt_output.contains("pasted-client-secret"),
            "prompt output must not contain the pasted secret"
        );

        let mut pasted = Vec::new();
        pasted.extend_from_slice(BRACKETED_PASTE_START);
        pasted.extend_from_slice(b"pasted-client-secret");
        pasted.extend_from_slice(BRACKETED_PASTE_END);
        pasted.push(b'\n');
        writer.write_all(&pasted)?;
        drop(writer);

        let status = child.wait()?;
        assert!(
            status.success(),
            "PTY hidden prompt child test failed: {status}"
        );
        Ok(())
    }

    /// PTY 上の実 hidden prompt でも、bracketed paste 本文の上限超過が固定 error で失敗する。
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    #[test]
    fn pty_hidden_prompt_rejects_over_limit_pasted_bracketed_input() -> Result<()> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut command = CommandBuilder::new(std::env::current_exe()?);
        command.args([
            "--exact",
            "secrets::support::process_io::tests::pty_hidden_child_rejects_over_limit_pasted_input",
            "--nocapture",
        ]);
        command.env(PTY_CHILD_ENV, "1");
        let mut child = pair.slave.spawn_command(command)?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let mut writer = pair.master.take_writer()?;
        let prompt_output = read_until_prompt(&mut *reader, "hidden-test: ")?;
        assert!(
            !prompt_output.contains("pasted-client-secret"),
            "prompt output must not contain the pasted secret"
        );

        let mut pasted = Vec::new();
        pasted.extend_from_slice(BRACKETED_PASTE_START);
        pasted.extend_from_slice(b"pasted-client-secret");
        pasted.extend_from_slice(BRACKETED_PASTE_END);
        pasted.push(b'\n');
        writer.write_all(&pasted)?;
        drop(writer);

        let status = child.wait()?;
        assert!(
            status.success(),
            "PTY hidden prompt over-limit child test failed: {status}"
        );
        Ok(())
    }

    /// `pty_hidden_prompt_accepts_pasted_bracketed_input` の子プロセス側で実 hidden reader を実行する。
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    #[test]
    fn pty_hidden_child_reads_pasted_input() -> Result<()> {
        if std::env::var_os(PTY_CHILD_ENV).is_none() {
            return Ok(());
        }

        let secret = read_hidden_line("hidden-test: ", 1024, TOO_LONG)?;
        assert_secret_bytes_eq(
            &secret.to_test_bytes(),
            b"pasted-client-secret",
            "pty pasted hidden input",
        );
        Ok(())
    }

    /// `pty_hidden_prompt_rejects_over_limit_pasted_bracketed_input` の子プロセス側で失敗経路を検証する。
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    #[test]
    fn pty_hidden_child_rejects_over_limit_pasted_input() -> Result<()> {
        if std::env::var_os(PTY_CHILD_ENV).is_none() {
            return Ok(());
        }

        assert_hidden_too_long(read_hidden_line("hidden-test: ", 4, TOO_LONG));
        Ok(())
    }

    /// stdout が PTY terminal の子プロセスで、`write_secret_stdout_with` が平文出力を拒否する境界を実行する。
    ///
    /// integration test は piped stdout（非 terminal）で成功側だけを通すため、terminal 拒否側を
    /// 実 terminal 上で一度実行する。子プロセスの PTY 出力に secret fixture が現れないことで、
    /// 拒否境界が writer を実行する前に停止することも確認する。
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    #[test]
    fn pty_write_secret_stdout_rejects_terminal() -> Result<()> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut command = CommandBuilder::new(std::env::current_exe()?);
        command.args([
            "--exact",
            "secrets::support::process_io::tests::pty_write_secret_child_rejects_terminal",
            "--nocapture",
        ]);
        command.env(PTY_CHILD_ENV, "1");
        let mut child = pair.slave.spawn_command(command)?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let mut output = Vec::new();
        reader.read_to_end(&mut output)?;
        let text = String::from_utf8_lossy(&output);
        assert!(
            !text.contains("terminal-rejected-secret"),
            "terminal-rejected path must not emit secret material to the terminal"
        );

        let status = child.wait()?;
        assert!(
            status.success(),
            "PTY terminal-rejection child test failed: {status}"
        );
        Ok(())
    }

    /// `pty_write_secret_stdout_rejects_terminal` の子プロセス側で terminal 拒否分岐を実行する。
    ///
    /// stdout が PTY slave に接続され terminal とみなされる前提で、`write_secret_stdout_with` が
    /// 固定メッセージで失敗し、secret writer を一度も実行しないことを検証する。
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    #[test]
    fn pty_write_secret_child_rejects_terminal() -> Result<()> {
        if std::env::var_os(PTY_CHILD_ENV).is_none() {
            return Ok(());
        }

        assert!(
            io::stdout().is_terminal(),
            "PTY child stdout must be a terminal for this assertion"
        );

        let mut writer_invoked = false;
        let result = write_secret_stdout_with(|stdout| {
            writer_invoked = true;
            stdout.write_all(b"terminal-rejected-secret")?;
            Ok(())
        });

        let error = match result {
            Ok(()) => panic!("write_secret_stdout_with must reject terminal stdout"),
            Err(error) => error,
        };
        assert!(
            !writer_invoked,
            "secret writer must not run when stdout is a terminal"
        );
        assert!(
            error
                .to_string()
                .contains("refusing to write secret to terminal"),
            "terminal rejection must use the fixed refusal message"
        );
        Ok(())
    }
}
