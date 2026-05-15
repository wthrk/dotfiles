//! `dotfiles secrets` の対話入力と stdin 入力を secret storage 型へ変換する。
//!
//! secret 本文は CLI 引数にせず、読み込み開始時点から zeroize 対象の buffer に置く。

use std::{
    io::{self, IsTerminal, Read, Write},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use serde::Deserialize;
use zeroize::{Zeroize, Zeroizing};

use super::{
    memory::{InterruptGuard, SecretMemoryGuard},
    storage::{self, BootstrapSecrets, SecretName, secret_name},
};
use crate::Result;

const MAX_BOOTSTRAP_JSON_LEN: usize = 64 * 1024;
const MAX_SINGLE_STDIN_SECRET_LEN: usize = 16 * 1024;

#[derive(Deserialize)]
/// `--stdin-json` で受け取る bootstrap secret 入力。
struct BootstrapSecretsJson {
    #[serde(rename = "bw-email")]
    bw_email: String,
    #[serde(rename = "bw-password")]
    bw_password: String,
    #[serde(rename = "bws-access-token")]
    bws_access_token: String,
}

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
        if input_len > MAX_BOOTSTRAP_JSON_LEN {
            bail!("bootstrap secret JSON input is too large");
        }
        input.truncate(input_len);
        let parsed: BootstrapSecretsJson =
            serde_json::from_slice(&input).context("failed to parse bootstrap secret JSON")?;
        input.zeroize();
        drop(input_lock);
        let secrets = BootstrapSecrets {
            bw_email: storage::secret_bytes(parsed.bw_email.into_bytes()),
            bw_password: storage::secret_bytes(parsed.bw_password.into_bytes()),
            bws_access_token: storage::secret_bytes(parsed.bws_access_token.into_bytes()),
        };
        return lock_bootstrap_secrets(secrets, memory);
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

/// 端末に表示しない prompt から 1 secret を読み取る。
pub(crate) fn read_hidden(prompt: &str) -> Result<Zeroizing<Vec<u8>>> {
    let value = rpassword::prompt_password(prompt)?;
    Ok(Zeroizing::new(value.into_bytes()))
}

/// rotate 対話実行で次の YubiKey も更新するか確認する。
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
    read_terminal_line_until(deadline, interrupt)
}

/// 低水準 `get` command の唯一の出力として secret bytes を標準出力へ渡す。
pub(crate) fn write_secret_to_stdout(secret: &[u8]) -> Result<()> {
    io::copy(&mut &*secret, &mut io::stdout())?;
    Ok(())
}

pub(crate) fn protect_zeroizing_secret(mut secret: Zeroizing<Vec<u8>>) -> storage::SecretBytes {
    storage::secret_bytes(std::mem::take(&mut *secret))
}

fn lock_bootstrap_secrets(
    secrets: BootstrapSecrets,
    memory: Option<&mut SecretMemoryGuard>,
) -> Result<BootstrapSecrets> {
    if let Some(memory) = memory {
        return memory.lock_bootstrap(secrets);
    }

    Ok(secrets)
}

/// stdin から単一 secret を読み、terminal 入力由来の末尾改行だけを正規化する。
fn read_one_stdin_secret() -> Result<Zeroizing<Vec<u8>>> {
    let mut input = Zeroizing::new(vec![0_u8; MAX_SINGLE_STDIN_SECRET_LEN + 1]);
    let input_len = read_stdin_into_fixed_buffer(
        &mut io::stdin(),
        &mut input,
        "stdin secret input is too large",
    )?;
    if input_len > MAX_SINGLE_STDIN_SECRET_LEN {
        bail!("stdin secret input is too large");
    }
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

/// spare 差し替え待ちは、terminal read の完了より timeout / interrupt を優先する。
fn read_terminal_line_until(deadline: Instant, interrupt: &InterruptGuard) -> Result<()> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut line = String::new();
        let result = io::stdin().read_line(&mut line);
        let _ = sender.send(result);
    });

    receive_terminal_line(deadline, receiver, interrupt)
}

/// `enroll-spare` の spare 差し替え待ちは、secret 保持中の timeout / interrupt 境界になる。
fn receive_terminal_line(
    deadline: Instant,
    receiver: mpsc::Receiver<io::Result<usize>>,
    interrupt: &InterruptGuard,
) -> Result<()> {
    loop {
        if interrupt.interrupted() {
            bail!("interrupted while handling bootstrap secrets");
        }
        let now = Instant::now();
        if now >= deadline {
            bail!("timed out waiting for spare YubiKey");
        }

        let poll_interval = Duration::from_millis(100).min(deadline.saturating_duration_since(now));
        match receiver.recv_timeout(poll_interval) {
            Ok(Ok(_)) => return Ok(()),
            Ok(Err(err)) if err.kind() == io::ErrorKind::Interrupted && interrupt.interrupted() => {
                bail!("interrupted while handling bootstrap secrets");
            }
            Ok(Err(err)) => return Err(err.into()),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                bail!("failed to read terminal input");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_one_trailing_newline() {
        let mut value = b"secret\n".to_vec();
        trim_one_trailing_newline(&mut value);
        assert_eq!(value, b"secret");

        let mut value = b"secret\n\n".to_vec();
        trim_one_trailing_newline(&mut value);
        assert_eq!(value, b"secret\n");
    }
}
