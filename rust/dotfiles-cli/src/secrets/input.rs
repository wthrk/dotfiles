//! `dotfiles secrets` の対話入力と stdin 入力を secret storage 型へ変換する。
//!
//! secret 本文は CLI 引数にせず、読み込み開始時点から zeroize 対象の buffer に置く。

use std::{
    io::{self, IsTerminal, Read, Write},
    os::fd::AsFd,
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use nix::{
    errno::Errno,
    poll::{PollFd, PollFlags, PollTimeout, poll},
    unistd,
};
use serde::Deserialize;
use zeroize::{Zeroize, Zeroizing};

use super::{
    memory::{InterruptGuard, SecretMemoryGuard},
    storage::{self, BootstrapSecrets, SecretName, secret_name},
};
use crate::Result;

const MAX_BOOTSTRAP_JSON_LEN: usize = 64 * 1024;
const MAX_SINGLE_STDIN_SECRET_LEN: usize = 16 * 1024;

/// `put` 用の secret を prompt または stdin から読み取る。
///
/// CLI 引数では secret 本文を受け取らない。stdin では末尾改行を 1 つだけ除去し、
/// それ以外の bytes は保存対象として保持する。
pub(crate) fn read_secret_for_put(name: SecretName, stdin: bool) -> Result<Zeroizing<Vec<u8>>> {
    if stdin {
        read_one_stdin_secret()
    } else {
        read_hidden(&format!("{}: ", secret_name(name)))
    }
}

/// bootstrap secret 一式を prompt または JSON stdin から読み取る。
///
/// prompt では email だけを表示入力にし、password と BWS token は hidden prompt で
/// 受け取る。`--stdin-json` は migration / recovery 用の非対話入口である。
pub(crate) fn read_bootstrap_secrets(
    stdin_json: bool,
    memory: Option<&mut SecretMemoryGuard>,
) -> Result<BootstrapSecrets> {
    if stdin_json {
        let mut input = Zeroizing::new(vec![0_u8; MAX_BOOTSTRAP_JSON_LEN + 1]);
        let input_lock = if let Some(memory) = memory.as_deref() {
            Some(memory.lock_transient_buffer(input.as_ptr(), input.len())?)
        } else {
            None
        };
        let input_len = read_stdin_into_fixed_buffer(
            &mut io::stdin(),
            &mut input,
            "bootstrap secret JSON input is too large",
        )?;
        input.truncate(input_len);
        let secrets = parse_bootstrap_secrets_json(&input, memory)
            .context("failed to parse bootstrap secret JSON")?;
        input.zeroize();
        drop(input_lock);
        return Ok(secrets);
    }

    let mut email = Zeroizing::new(String::new());
    eprint!("bw-email: ");
    io::stderr().flush()?;
    io::stdin().read_line(&mut email)?;
    let mut email = std::mem::take(&mut *email).into_bytes();
    trim_one_trailing_newline(&mut email);

    let secrets = BootstrapSecrets {
        bw_email: storage::secret_bytes(email),
        bw_password: protect_zeroizing_secret(read_hidden("bw-password: ")?),
        bws_access_token: protect_zeroizing_secret(read_hidden("bws-access-token: ")?),
    };
    lock_bootstrap_secrets(secrets, memory)
}

/// hidden prompt の戻り値は、CLI 側へ渡す前から zeroize 対象の bytes として保持する。
pub(crate) fn read_hidden(prompt: &str) -> Result<Zeroizing<Vec<u8>>> {
    let value = rpassword::prompt_password(prompt)?;
    Ok(Zeroizing::new(value.into_bytes()))
}

/// 非対話 stdin では追加更新を prompt できないため、複数本更新を開始しない。
pub(crate) fn prompt_yes_no(prompt: &str) -> Result<bool> {
    if !io::stdin().is_terminal() {
        return Ok(false);
    }

    eprint!("{prompt}");
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(
        input.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// YubiKey 差し替え prompt の Enter 入力を待つ。
///
/// stdin が terminal でない場合は prompt による同期ができないため失敗させる。
pub(crate) fn wait_for_enter(deadline: Instant, interrupt: &InterruptGuard) -> Result<()> {
    if !io::stdin().is_terminal() {
        bail!("pass --spare-serial in non-interactive use");
    }
    read_terminal_line_until(deadline, interrupt).map(|_| ())
}

/// 低水準 `get` command の唯一の出力として secret bytes を pipe / redirect へ渡す。
///
/// terminal へ直接出す実行は、平文 secret が画面や scrollback に残るため拒否する。
pub(crate) fn write_secret_to_stdout(secret: &[u8]) -> Result<()> {
    if io::stdout().is_terminal() {
        bail!("refusing to write secret to terminal; redirect stdout to a file or pipe");
    }
    io::copy(&mut &*secret, &mut io::stdout())?;
    Ok(())
}

/// prompt/stdin の一時 buffer から secret wrapper へ所有権を移し、元 buffer の Drop で残存を消す。
pub(crate) fn protect_zeroizing_secret(mut secret: Zeroizing<Vec<u8>>) -> storage::SecretBytes {
    storage::secret_bytes(std::mem::take(&mut *secret))
}

/// memory guard がある登録経路では、3 secret すべてを同じ process/memory 保護下に置く。
fn lock_bootstrap_secrets(
    secrets: BootstrapSecrets,
    memory: Option<&mut SecretMemoryGuard>,
) -> Result<BootstrapSecrets> {
    if let Some(memory) = memory {
        return memory.lock_bootstrap(secrets);
    }

    Ok(secrets)
}

/// `--stdin-json` の構造検証は serde に任せ、decode 後の field 文字列を zeroize 対象にする。
fn parse_bootstrap_secrets_json(
    input: &[u8],
    memory: Option<&mut SecretMemoryGuard>,
) -> Result<BootstrapSecrets> {
    let input: BootstrapSecretsInput = serde_json::from_slice(input)?;
    lock_bootstrap_secrets(input.into_bootstrap_secrets(), memory)
}

#[derive(Deserialize)]
struct BootstrapSecretsInput {
    #[serde(rename = "bw-email")]
    bw_email: Zeroizing<String>,
    #[serde(rename = "bw-password")]
    bw_password: Zeroizing<String>,
    #[serde(rename = "bws-access-token")]
    bws_access_token: Zeroizing<String>,
}

impl BootstrapSecretsInput {
    fn into_bootstrap_secrets(self) -> BootstrapSecrets {
        BootstrapSecrets {
            bw_email: secret_string_bytes(self.bw_email),
            bw_password: secret_string_bytes(self.bw_password),
            bws_access_token: secret_string_bytes(self.bws_access_token),
        }
    }
}

fn secret_string_bytes(mut secret: Zeroizing<String>) -> storage::SecretBytes {
    storage::secret_bytes(std::mem::take(&mut *secret).into_bytes())
}

/// stdin から単一 secret を読み、terminal 入力由来の末尾改行だけを正規化する。
fn read_one_stdin_secret() -> Result<Zeroizing<Vec<u8>>> {
    let mut input = Zeroizing::new(vec![0_u8; MAX_SINGLE_STDIN_SECRET_LEN + 1]);
    let input_len = read_stdin_into_fixed_buffer(
        &mut io::stdin(),
        &mut input,
        "stdin secret input is too large",
    )?;
    input.truncate(input_len);
    trim_one_trailing_newline(&mut input);
    Ok(input)
}

/// 事前確保した buffer の範囲だけへ読み込み、上限超過を追加確保なしで検出する。
fn read_stdin_into_fixed_buffer<R: Read>(
    reader: &mut R,
    buffer: &mut [u8],
    oversized_error: &str,
) -> Result<usize> {
    let mut total_len = 0;
    while total_len < buffer.len() {
        let read_len = reader.read(&mut buffer[total_len..])?;
        if read_len == 0 {
            return Ok(total_len);
        }
        total_len += read_len;
    }

    bail!("{oversized_error}");
}

/// stdin/prompt 由来の入力で混入しやすい末尾 newline 1 個だけを保存対象から外す。
fn trim_one_trailing_newline(input: &mut Vec<u8>) {
    if input.ends_with(b"\n") {
        input.pop();
        if input.ends_with(b"\r") {
            input.pop();
        }
    }
}

/// spare 差し替え待ちは同一 thread で 1 byte ずつ読み、timeout 後に入力消費を残さない。
pub(crate) fn read_terminal_line_until(
    deadline: Instant,
    interrupt: &InterruptGuard,
) -> Result<String> {
    let stdin = io::stdin();
    let mut bytes = Vec::new();
    loop {
        let timeout = next_terminal_poll_timeout(deadline, interrupt)?;

        let mut fds = [PollFd::new(stdin.as_fd(), PollFlags::POLLIN)];
        match poll(&mut fds, timeout) {
            Ok(0) => {}
            Ok(_) => {
                let mut byte = [0_u8; 1];
                let read_len = match unistd::read(stdin.as_fd(), &mut byte) {
                    Ok(read_len) => read_len,
                    Err(Errno::EINTR) => continue,
                    Err(err) => return Err(io::Error::from_raw_os_error(err as i32).into()),
                };
                if read_len == 0 {
                    bail!("failed to read terminal input");
                }
                bytes.push(byte[0]);
                if byte[0] == b'\n' {
                    let line =
                        String::from_utf8(bytes).context("terminal input is not valid UTF-8")?;
                    return Ok(line);
                }
            }
            Err(Errno::EINTR) => {}
            Err(err) => return Err(io::Error::from_raw_os_error(err as i32).into()),
        }
    }
}

/// 対話入力待ちループは中断と期限超過を poll 前に確定し、待機時間だけを返す。
fn next_terminal_poll_timeout(
    deadline: Instant,
    interrupt: &InterruptGuard,
) -> Result<PollTimeout> {
    if interrupt.interrupted() {
        bail!("interrupted while handling bootstrap secrets");
    }
    let now = Instant::now();
    if now >= deadline {
        bail!("timed out waiting for spare YubiKey");
    }

    let poll_interval = Duration::from_millis(100).min(deadline.saturating_duration_since(now));
    PollTimeout::try_from(poll_interval)
        .map_err(|_| anyhow::anyhow!("failed to convert terminal poll timeout"))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use secrecy::ExposeSecret;

    #[test]
    fn parses_bootstrap_secrets_json() -> Result<()> {
        let input = br#"{"bw-email":"u@example.com","bw-password":"pw","bws-access-token":"tok"}"#;
        let secrets = parse_bootstrap_secrets_json(input, None)?;
        assert_eq!(secrets.bw_email.expose_secret(), b"u@example.com");
        assert_eq!(secrets.bw_password.expose_secret(), b"pw");
        assert_eq!(secrets.bws_access_token.expose_secret(), b"tok");
        Ok(())
    }

    #[test]
    fn rejects_bootstrap_secrets_json_with_missing_field() {
        let input = br#"{"bw-email":"u@example.com","bw-password":"pw"}"#;
        assert!(parse_bootstrap_secrets_json(input, None).is_err());
    }

    #[test]
    fn rejects_bootstrap_secrets_json_with_duplicate_field() {
        let input =
            br#"{"bw-email":"a","bw-email":"b","bw-password":"pw","bws-access-token":"tok"}"#;
        assert!(parse_bootstrap_secrets_json(input, None).is_err());
    }

    #[test]
    fn rejects_bootstrap_secrets_json_with_non_string_field() {
        let input = br#"{"bw-email":"u@example.com","bw-password":123,"bws-access-token":"tok"}"#;
        assert!(parse_bootstrap_secrets_json(input, None).is_err());
    }

    #[test]
    fn trims_one_trailing_newline() {
        let mut value = b"secret\n".to_vec();
        trim_one_trailing_newline(&mut value);
        assert_eq!(value, b"secret");

        let mut value = b"secret\n\n".to_vec();
        trim_one_trailing_newline(&mut value);
        assert_eq!(value, b"secret\n");
    }

    #[test]
    fn fixed_buffer_reader_rejects_oversized_input() {
        let mut reader = io::Cursor::new(vec![b'x'; 5]);
        let mut buffer = vec![0_u8; 4];
        let result = read_stdin_into_fixed_buffer(&mut reader, &mut buffer, "too large");
        assert!(result.is_err());
    }

    #[test]
    fn fixed_buffer_reader_accepts_input_when_sentinel_slot_remains() -> Result<()> {
        let mut reader = io::Cursor::new(vec![b'x'; 3]);
        let mut buffer = vec![0_u8; 4];
        let len = read_stdin_into_fixed_buffer(&mut reader, &mut buffer, "too large")?;
        assert_eq!(len, 3);
        Ok(())
    }

    #[test]
    fn terminal_poll_timeout_rejects_expired_deadline() -> Result<()> {
        let guard = InterruptGuard::install()?;
        let result = next_terminal_poll_timeout(Instant::now(), &guard);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn terminal_poll_timeout_rejects_interrupt_before_poll() -> Result<()> {
        let guard = InterruptGuard::install()?;
        signal_hook::low_level::raise(signal_hook::consts::signal::SIGINT)?;
        let result = next_terminal_poll_timeout(Instant::now() + Duration::from_secs(1), &guard);
        assert!(result.is_err());
        Ok(())
    }
}
