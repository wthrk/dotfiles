//! SSH agent wire protocol と sshcontrol の process-generic technical operations。

use crate::{Result, features::gpg_backup_recovery::domain::gpg_restore::Keygrip};
use anyhow::Context;
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::{
    fs::OpenOptions,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
};
#[cfg(unix)]
const SSH_AGENTC_REQUEST_IDENTITIES: u8 = 11;
#[cfg(unix)]
const SSH_AGENT_IDENTITIES_ANSWER: u8 = 12;

#[cfg(unix)]
pub(crate) fn request_identities(socket: &Path) -> Result<Vec<Vec<u8>>> {
    let mut stream =
        UnixStream::connect(socket).context("failed to connect to SSH agent socket")?;
    stream
        .write_all(&[0, 0, 0, 1, SSH_AGENTC_REQUEST_IDENTITIES])
        .context("failed to send SSH agent identities request")?;
    stream
        .flush()
        .context("failed to flush SSH agent identities request")?;
    let mut length = [0u8; 4];
    stream
        .read_exact(&mut length)
        .context("failed to read SSH agent response length")?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > 1 << 20 {
        anyhow::bail!("SSH agent response length is out of range");
    }
    let mut payload = vec![0; length];
    stream
        .read_exact(&mut payload)
        .context("failed to read SSH agent response payload")?;
    let mut cursor = Cursor {
        bytes: &payload,
        offset: 0,
    };
    if cursor.u8()? != SSH_AGENT_IDENTITIES_ANSWER {
        anyhow::bail!("unexpected SSH agent response message type");
    }
    let count = cursor.u32()?;
    let mut keys = Vec::with_capacity(count as usize);
    for _ in 0..count {
        keys.push(cursor.string()?.to_vec());
        let _ = cursor.string()?;
    }
    Ok(keys)
}
#[cfg(unix)]
struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}
#[cfg(unix)]
impl<'a> Cursor<'a> {
    fn u8(&mut self) -> Result<u8> {
        let value = *self
            .bytes
            .get(self.offset)
            .context("SSH agent response is truncated")?;
        self.offset += 1;
        Ok(value)
    }
    fn u32(&mut self) -> Result<u32> {
        let end = self
            .offset
            .checked_add(4)
            .filter(|end| *end <= self.bytes.len())
            .context("SSH agent response is truncated")?;
        let bytes: [u8; 4] = self.bytes[self.offset..end]
            .try_into()
            .context("SSH agent response has an invalid u32 field")?;
        let value = u32::from_be_bytes(bytes);
        self.offset = end;
        Ok(value)
    }
    fn string(&mut self) -> Result<&'a [u8]> {
        let len = self.u32()? as usize;
        let end = self
            .offset
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .context("SSH agent response string is truncated")?;
        let value = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(value)
    }
}
pub(crate) fn sshcontrol_path(home: PathBuf) -> PathBuf {
    home.join("sshcontrol")
}
pub(crate) fn sshcontrol_contains(path: &Path, keygrip: &Keygrip) -> Result<bool> {
    let file = match OpenOptions::new().read(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(anyhow::Error::new(error).context("failed to read gpg-agent sshcontrol"));
        }
    };
    for line in BufReader::new(file).lines() {
        let line = line.context("failed to read gpg-agent sshcontrol line")?;
        if sshcontrol_line_matches_keygrip(&line, keygrip) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `sshcontrol` の一行が対象 keygrip を登録しているかを判定する。
pub(crate) fn sshcontrol_line_matches_keygrip(line: &str, keygrip: &Keygrip) -> bool {
    let entry = line.trim();
    !entry.is_empty()
        && !entry.starts_with('#')
        && entry
            .split_whitespace()
            .next()
            .unwrap_or(entry)
            .eq_ignore_ascii_case(keygrip.as_str())
}
