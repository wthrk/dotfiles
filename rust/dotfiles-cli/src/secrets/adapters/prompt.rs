//! terminal prompt からの secret 読み取り adapter。
//!
//! visible prompt、hidden prompt、YubiKey PIN 入力を扱う。

use std::{
    io::{self, Read, Write},
    sync::mpsc,
    thread,
    time::Duration,
};

use anyhow::bail;

use super::terminal;
use crate::{
    secrets::support::protection::{ProtectedInputBuffer, ProtectedSecret, SecretSession},
    Result,
};

const PIV_PIN_MIN_LEN: usize = 6;
const PIV_PIN_MAX_LEN: usize = 8;

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
