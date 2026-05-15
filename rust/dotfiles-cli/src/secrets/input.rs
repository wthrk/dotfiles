//! `dotfiles secrets` の対話入力と stdin 入力を secret storage 型へ変換する。
//!
//! secret 本文は CLI 引数にせず、読み込み開始時点から zeroize 対象の buffer に置く。

use std::{
    io::{self, IsTerminal, Read, Write},
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
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
pub(crate) const SPARE_SERIAL_NONINTERACTIVE_ERROR: &str =
    "pass --spare-serial in non-interactive use";
pub(crate) const SPARE_WAIT_TIMEOUT_ERROR: &str = "timed out waiting for spare YubiKey";

/// 対話選択で利用者へ表示する YubiKey 候補。
pub(crate) struct YubikeySelectionCandidate<'a> {
    pub(crate) reader: &'a str,
    pub(crate) serial: u32,
}

/// 非対話実行の precondition を command / device 境界から同じ判定へ集約する。
pub(crate) fn stdin_is_terminal() -> bool {
    io::stdin().is_terminal()
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
        let mut input = Zeroizing::new(Vec::with_capacity(MAX_BOOTSTRAP_JSON_LEN + 1));
        let input_lock = if let Some(memory) = memory.as_deref() {
            Some(memory.lock_transient_buffer(input.as_ptr(), input.capacity())?)
        } else {
            None
        };
        let read_result = io::stdin()
            .take((MAX_BOOTSTRAP_JSON_LEN + 1) as u64)
            .read_to_end(&mut input);
        let secrets = read_result.map_err(anyhow::Error::from).and_then(|_| {
            if input.len() > MAX_BOOTSTRAP_JSON_LEN {
                bail!("bootstrap secret JSON input is too large");
            }
            parse_bootstrap_secrets_json(&input, memory)
                .context("failed to parse bootstrap secret JSON")
        });
        input.zeroize();
        drop(input_lock);
        return secrets;
    }

    let email_bytes = read_visible_secret_line("bw-email: ")?;

    if let Some(memory) = memory {
        return Ok(BootstrapSecrets {
            bw_email: memory
                .lock_secret(SecretName::BwEmail, storage::secret_bytes(email_bytes))?,
            bw_password: memory.lock_secret(
                SecretName::BwPassword,
                protect_zeroizing_secret(read_hidden("bw-password: ")?),
            )?,
            bws_access_token: memory.lock_secret(
                SecretName::BwsAccessToken,
                protect_zeroizing_secret(read_hidden("bws-access-token: ")?),
            )?,
        });
    }

    Ok(BootstrapSecrets {
        bw_email: storage::secret_bytes(email_bytes),
        bw_password: protect_zeroizing_secret(read_hidden("bw-password: ")?),
        bws_access_token: protect_zeroizing_secret(read_hidden("bws-access-token: ")?),
    })
}

/// 表示入力の secret line は読み取り直後に bytes 化し、末尾改行だけを保存対象から外す。
fn read_visible_secret_line(prompt: &str) -> Result<Vec<u8>> {
    let mut line = Zeroizing::new(String::new());
    eprint!("{prompt}");
    io::stderr().flush()?;
    io::stdin().read_line(&mut line)?;
    let mut bytes = std::mem::take(&mut *line).into_bytes();
    trim_one_trailing_newline(&mut bytes);
    Ok(bytes)
}

/// hidden prompt の戻り値は、CLI 側へ渡す前から zeroize 対象の bytes として保持する。
pub(crate) fn read_hidden(prompt: &str) -> Result<Zeroizing<Vec<u8>>> {
    let value = rpassword::prompt_password(prompt)?;
    Ok(Zeroizing::new(value.into_bytes()))
}

/// YubiKey PIV private operation の PIN を hidden prompt で受け取る。
pub(crate) fn read_yubikey_pin() -> Result<Zeroizing<Vec<u8>>> {
    read_hidden("YubiKey PIN: ")
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
        bail!(SPARE_SERIAL_NONINTERACTIVE_ERROR);
    }
    read_terminal_line_until(deadline, interrupt).map(|_| ())
}

/// primary と同じ YubiKey が選ばれた場合だけ、spare への差し替えを対話で同期する。
pub(crate) fn wait_for_spare_replacement(
    deadline: Instant,
    interrupt: &InterruptGuard,
) -> Result<()> {
    eprintln!("The selected YubiKey is the primary; replace it with the spare.");
    eprintln!("Insert the spare YubiKey, then press Enter.");
    wait_for_enter(deadline, interrupt)
}

/// 複数 YubiKey 候補を表示し、利用者が選んだ候補 index を返す。
pub(crate) fn select_yubikey_candidate(
    candidates: &[YubikeySelectionCandidate<'_>],
    timed_input: Option<(Instant, &InterruptGuard)>,
) -> Result<usize> {
    if !io::stdin().is_terminal() {
        bail!("multiple YubiKeys detected; pass a serial option in non-interactive use");
    }

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
    io::stderr().flush()?;

    let input = if let Some((deadline, interrupt)) = timed_input {
        read_terminal_line_until(deadline, interrupt)?
    } else {
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        input
    };
    let selected = input.trim().parse::<usize>().context("invalid selection")?;
    if selected == 0 || selected > candidates.len() {
        bail!("selected YubiKey is out of range");
    }

    Ok(selected - 1)
}

/// 低水準 `get` command の唯一の出力として secret bytes を pipe / redirect へ渡す。
///
/// terminal へ直接出す実行は、平文 secret が画面や scrollback に残るため拒否する。
pub(crate) fn write_secret_to_stdout(bytes: &[u8]) -> Result<()> {
    if io::stdout().is_terminal() {
        bail!("refusing to write secret to terminal; redirect stdout to a file or pipe");
    }
    io::stdout().lock().write_all(bytes)?;
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
        return Ok(BootstrapSecrets {
            bw_email: memory.lock_secret(SecretName::BwEmail, secrets.bw_email)?,
            bw_password: memory.lock_secret(SecretName::BwPassword, secrets.bw_password)?,
            bws_access_token: memory
                .lock_secret(SecretName::BwsAccessToken, secrets.bws_access_token)?,
        });
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
    /// JSON field ごとの `Zeroizing<String>` から storage 用 secret bytes へ所有権を移す。
    fn into_bootstrap_secrets(self) -> BootstrapSecrets {
        BootstrapSecrets {
            bw_email: secret_string_bytes(self.bw_email),
            bw_password: secret_string_bytes(self.bw_password),
            bws_access_token: secret_string_bytes(self.bws_access_token),
        }
    }
}

/// serde が生成した string buffer を再確保せず bytes に変換し、元 string の Drop で残存を消す。
fn secret_string_bytes(mut secret: Zeroizing<String>) -> storage::SecretBytes {
    storage::secret_bytes(std::mem::take(&mut *secret).into_bytes())
}

/// stdin から単一 secret を読み、terminal 入力由来の末尾改行だけを正規化する。
fn read_one_stdin_secret() -> Result<Zeroizing<Vec<u8>>> {
    let mut input = Vec::with_capacity(MAX_SINGLE_STDIN_SECRET_LEN + 1);
    io::stdin()
        .take((MAX_SINGLE_STDIN_SECRET_LEN + 1) as u64)
        .read_to_end(&mut input)?;
    if input.len() > MAX_SINGLE_STDIN_SECRET_LEN {
        bail!("stdin secret input is too large");
    }

    trim_one_trailing_newline(&mut input);
    Ok(Zeroizing::new(input))
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

/// spare 差し替え待ちは端末 event API で読み、timeout 後に stdin 待ち thread を残さない。
pub(crate) fn read_terminal_line_until(
    deadline: Instant,
    interrupt: &InterruptGuard,
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
            bail!(SPARE_WAIT_TIMEOUT_ERROR);
        }
        let timeout = Duration::from_millis(100).min(deadline.saturating_duration_since(now));

        if !event::poll(timeout)? {
            continue;
        }

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        match key.code {
            KeyCode::Enter => {
                eprintln!();
                return Ok(line);
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
    }
}

#[cfg(test)]
mod tests {
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

        let mut value = b"secret\r\n".to_vec();
        trim_one_trailing_newline(&mut value);
        assert_eq!(value, b"secret");
    }
}
