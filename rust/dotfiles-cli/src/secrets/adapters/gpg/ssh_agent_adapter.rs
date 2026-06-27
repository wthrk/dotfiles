//! `SshAgentPort` を gpg-agent の SSH key list（`sshcontrol`）と SSH agent socket 観測へ接続する adapter。
//!
//! authentication subkey の keygrip を `${GNUPGHOME:-$HOME/.gnupg}/sshcontrol` へ冪等に登録し、SSH support
//! 利用可否を「SSH agent socket（`S.gpg-agent.ssh`）が解決でき、その socket 経路で agent が列挙する identity に
//! 復元鍵が含まれる」状態として観測して domain 値（`SshAgentReadiness`）へ翻訳する。`gpgconf` CLI は使わず、
//! socket は `${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` を優先候補として確認し、その path が socket でない場合
//! だけ既存環境変数 `SSH_AUTH_SOCK` が指す socket へ fallback する（`config/zsh/env.zsh` の上書き条件と同じ前提）。
//! identity の列挙は、解決した socket へ接続して SSH agent protocol（`SSH_AGENTC_REQUEST_IDENTITIES`）で公開鍵
//! identity を列挙し、各 identity の key blob を期待公開鍵（`OpenSshPublicKey`）の key blob と byte 一致で照合して、
//! 復元鍵 identity が識別可能かを判定する。復元鍵と無関係な既存 identity の有無は観測しない。
//! identity comment（gpg-agent は `cardno:` / `openpgp:` 等を載せ keygrip とは限らない）は鍵同一性に使えないため
//! 照合に用いない。SSH support 充足の業務判定そのものは domain（`SshAgentReadiness::ensure_ready` /
//! `OpenSshPublicKey::matches_agent_key_blob`）へ残す。

use std::{
    fs::OpenOptions,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::net::UnixStream;

use anyhow::Context;

use crate::{
    Result,
    secrets::{
        domain::gpg_restore::{Keygrip, OpenSshPublicKey, SshAgentReadiness},
        ports::gpg::SshAgentPort,
        support::ssh_agent_socket::{gnupg_home, resolve_ssh_agent_socket},
    },
};

/// gpg-agent の SSH key list と socket 観測を `SshAgentPort` 契約へ翻訳する adapter。
#[derive(Default)]
pub(super) struct SshAgentAdapter;

impl SshAgentPort for SshAgentAdapter {
    fn register_authentication_subkey(&mut self, keygrip: &Keygrip) -> Result<()> {
        let path = sshcontrol_path()?;
        if sshcontrol_contains(&path, keygrip)? {
            // 既登録ならその状態を維持する（冪等）。
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .context("failed to create GnuPG home directory for sshcontrol")?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .context("failed to open gpg-agent sshcontrol")?;
        writeln!(file, "{}", keygrip.as_str())
            .context("failed to register keygrip in gpg-agent sshcontrol")?;
        Ok(())
    }

    fn inspect_ssh_agent(
        &mut self,
        expected_public_key: &OpenSshPublicKey,
    ) -> Result<SshAgentReadiness> {
        // 固定 path が socket ならそれ、socket でなければ既存 `SSH_AUTH_SOCK` が socket ならそれを使う。
        // `gnupg_home()` 解決失敗（例: `HOME` 未設定）は socket 無しへ握り潰さず、その実原因を `?` で伝播する。
        let socket = resolve_ssh_agent_socket()?;
        let socket_resolved = socket.is_some();
        // 解決した socket へ接続して SSH agent protocol で公開鍵 identity を列挙し、各 identity の key blob を
        // 期待公開鍵の key blob と byte 一致で照合する。一致する identity があれば復元鍵が識別可能であり、
        // socket が解決できない、または接続/列挙に失敗した場合は false とし、復元鍵を識別できないことで
        // 停止させる（識別不能を「識別可能」へ倒さない）。復元鍵と無関係な既存 identity の
        // 有無は観測しない。
        let recovery_identity_present = match socket {
            Some(path) => inspect_agent_identities(&path, expected_public_key)?,
            None => false,
        };
        Ok(SshAgentReadiness {
            socket_resolved,
            recovery_identity_present,
        })
    }
}

/// SSH agent protocol で identity を列挙し、復元鍵 identity が識別可能かを返す。
///
/// 解決済み socket へ接続し、`SSH_AGENTC_REQUEST_IDENTITIES` を送って `SSH_AGENT_IDENTITIES_ANSWER` を
/// 解析する。各 identity の key blob を期待公開鍵（`OpenSshPublicKey`）の key blob と byte 一致で照合し、
/// 一致する identity があれば復元鍵が識別可能と判定する。復元鍵と無関係な既存 identity の
/// 有無は観測しない。gpg-agent の identity comment は keygrip とは限らない（`cardno:` / `openpgp:` 等）ため
/// 照合に用いない。接続/送受信/解析に失敗した場合は、socket への接続可否を最低限の観測代替とせず、停止条件を
/// 弱めないため `Err` を返して呼び出し側で false に倒す。
#[cfg(unix)]
fn inspect_agent_identities(socket: &Path, expected_public_key: &OpenSshPublicKey) -> Result<bool> {
    let key_blobs = request_ssh_identities(socket)?;
    Ok(key_blobs
        .iter()
        .any(|key_blob| expected_public_key.matches_agent_key_blob(key_blob)))
}

#[cfg(not(unix))]
fn inspect_agent_identities(
    _socket: &Path,
    _expected_public_key: &OpenSshPublicKey,
) -> Result<bool> {
    Ok(false)
}

/// SSH agent protocol の message 種別（必要な値のみ）。
#[cfg(unix)]
const SSH_AGENTC_REQUEST_IDENTITIES: u8 = 11;
#[cfg(unix)]
const SSH_AGENT_IDENTITIES_ANSWER: u8 = 12;

/// 解決済み socket へ接続し、列挙された identity の key blob bytes を返す。
///
/// SSH agent protocol は length-prefixed frame（先頭 4 byte の big-endian length に payload が続く）で、
/// `SSH_AGENT_IDENTITIES_ANSWER` の payload は `count`（u32）に続いて `(key_blob string, comment string)`
/// が count 回並ぶ。各 string は 4 byte 長 prefix 付きである。鍵同一性照合に使う key blob だけを取り出し、
/// secret material は要求・受信しない。
#[cfg(unix)]
fn request_ssh_identities(socket: &Path) -> Result<Vec<Vec<u8>>> {
    let mut stream =
        UnixStream::connect(socket).context("failed to connect to SSH agent socket")?;
    // request payload は message 種別 1 byte のみ。frame は 4 byte length prefix を付ける。
    let request = [0u8, 0, 0, 1, SSH_AGENTC_REQUEST_IDENTITIES];
    stream
        .write_all(&request)
        .context("failed to send SSH agent identities request")?;
    stream
        .flush()
        .context("failed to flush SSH agent identities request")?;

    let payload = read_agent_frame(&mut stream)?;
    parse_identities_answer(&payload)
}

/// SSH agent の 1 frame（4 byte length prefix + payload）を読み取り payload bytes を返す。
#[cfg(unix)]
fn read_agent_frame(stream: &mut UnixStream) -> Result<Vec<u8>> {
    let mut length_bytes = [0u8; 4];
    stream
        .read_exact(&mut length_bytes)
        .context("failed to read SSH agent response length")?;
    let length = u32::from_be_bytes(length_bytes) as usize;
    // 想定外に巨大な length は不正 frame として停止する（reply の妥当範囲を超える応答を受け取らない）。
    if length == 0 || length > 1 << 20 {
        anyhow::bail!("SSH agent response length is out of range");
    }
    let mut payload = vec![0u8; length];
    stream
        .read_exact(&mut payload)
        .context("failed to read SSH agent response payload")?;
    Ok(payload)
}

/// `SSH_AGENT_IDENTITIES_ANSWER` payload を解析し、各 identity の key blob bytes を返す。
#[cfg(unix)]
fn parse_identities_answer(payload: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut cursor = ByteCursor::new(payload);
    let message_type = cursor.take_u8()?;
    if message_type != SSH_AGENT_IDENTITIES_ANSWER {
        anyhow::bail!("unexpected SSH agent response message type");
    }
    let count = cursor.take_u32()?;
    let mut key_blobs = Vec::with_capacity(count as usize);
    for _ in 0..count {
        // 鍵同一性照合に使う key blob を取り出し、comment は識別に使わないため読み飛ばす。
        let key_blob = cursor.take_string()?;
        let _comment = cursor.take_string()?;
        key_blobs.push(key_blob.to_vec());
    }
    Ok(key_blobs)
}

/// SSH agent protocol payload を big-endian で順次読む内部カーソル。
#[cfg(unix)]
struct ByteCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

#[cfg(unix)]
impl<'a> ByteCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take_u8(&mut self) -> Result<u8> {
        let byte = *self
            .bytes
            .get(self.offset)
            .context("SSH agent response is truncated")?;
        self.offset += 1;
        Ok(byte)
    }

    fn take_u32(&mut self) -> Result<u32> {
        let end = self
            .offset
            .checked_add(4)
            .filter(|end| *end <= self.bytes.len())
            .context("SSH agent response is truncated")?;
        let value = u32::from_be_bytes([
            self.bytes[self.offset],
            self.bytes[self.offset + 1],
            self.bytes[self.offset + 2],
            self.bytes[self.offset + 3],
        ]);
        self.offset = end;
        Ok(value)
    }

    fn take_string(&mut self) -> Result<&'a [u8]> {
        let length = self.take_u32()? as usize;
        let end = self
            .offset
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .context("SSH agent response string is truncated")?;
        let slice = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(slice)
    }
}

/// gpg-agent の SSH key list（`sshcontrol`）の path を返す。
fn sshcontrol_path() -> Result<PathBuf> {
    Ok(gnupg_home()?.join("sshcontrol"))
}

/// `sshcontrol` に keygrip（uppercase hex 40）が既に登録されているかを返す。
fn sshcontrol_contains(path: &PathBuf, keygrip: &Keygrip) -> Result<bool> {
    let file = match OpenOptions::new().read(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error).context("failed to read gpg-agent sshcontrol");
        }
    };
    for line in BufReader::new(file).lines() {
        let line = line.context("failed to read gpg-agent sshcontrol line")?;
        let entry = line.trim();
        if entry.is_empty() || entry.starts_with('#') {
            continue;
        }
        // sshcontrol の各行は keygrip（uppercase hex）で始まり、`KEYGRIP 0 confirm` のように
        // オプションが続く場合がある。行全体一致ではなく先頭空白区切りトークンを keygrip と照合する。
        let token = entry.split_whitespace().next().unwrap_or(entry);
        if token.eq_ignore_ascii_case(keygrip.as_str()) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(all(test, unix))]
mod tests {
    //! SSH agent protocol の identity 応答解析と key blob 照合という adapter 翻訳ロジックの単体テスト。
    //!
    //! socket 接続を伴わない純粋な byte decode と key blob 照合だけを検証し、外部 agent は呼ばない。
    //! 期待公開鍵は base64 本体が agent key blob を base64 化したものである関係を使い、blob 一致/不一致を確認する。

    use super::{ByteCursor, SSH_AGENT_IDENTITIES_ANSWER, parse_identities_answer};
    use crate::secrets::domain::gpg_restore::OpenSshPublicKey;

    /// 長さ prefix 付き string を big-endian frame として連結する補助。
    fn push_string(buffer: &mut Vec<u8>, value: &[u8]) {
        buffer.extend_from_slice(&(value.len() as u32).to_be_bytes());
        buffer.extend_from_slice(value);
    }

    /// `(key_blob, comment)` を count 件持つ `SSH_AGENT_IDENTITIES_ANSWER` payload を組み立てる。
    fn identities_answer(identities: &[(&[u8], &[u8])]) -> Vec<u8> {
        let mut payload = vec![SSH_AGENT_IDENTITIES_ANSWER];
        payload.extend_from_slice(&(identities.len() as u32).to_be_bytes());
        for (key_blob, comment) in identities {
            push_string(&mut payload, key_blob);
            push_string(&mut payload, comment);
        }
        payload
    }

    #[test]
    fn parses_identity_key_blobs_in_order() -> crate::Result<()> {
        let payload = identities_answer(&[(b"blob-a", b"comment-a"), (b"blob-b", b"comment-b")]);
        let key_blobs = parse_identities_answer(&payload)?;
        assert_eq!(key_blobs, vec![b"blob-a".to_vec(), b"blob-b".to_vec()]);
        Ok(())
    }

    #[test]
    fn rejects_unexpected_message_type() {
        // 先頭 byte を別 message 種別にした応答は停止する。
        let mut payload = identities_answer(&[]);
        payload[0] = 99;
        assert!(parse_identities_answer(&payload).is_err());
    }

    #[test]
    fn rejects_truncated_string() {
        // string 長 prefix が payload 残量を超える応答は停止する。
        let mut payload = vec![SSH_AGENT_IDENTITIES_ANSWER];
        payload.extend_from_slice(&1u32.to_be_bytes());
        payload.extend_from_slice(&8u32.to_be_bytes());
        payload.extend_from_slice(b"short");
        assert!(parse_identities_answer(&payload).is_err());
    }

    #[test]
    fn identifies_public_key_by_parsed_key_blob() -> crate::Result<()> {
        // base64("blob") == "YmxvYg=="。期待公開鍵の key blob と一致する identity だけを識別する。
        let expected =
            OpenSshPublicKey::parse("ssh-ed25519 YmxvYg== cardno:0006").expect("valid key");
        let payload = identities_answer(&[(b"other", b"x"), (b"blob", b"cardno:0006")]);
        let key_blobs = parse_identities_answer(&payload)?;
        assert!(
            key_blobs
                .iter()
                .any(|key_blob| expected.matches_agent_key_blob(key_blob))
        );
        // 期待 blob を含まない応答は識別しない（comment ではなく blob で照合する）。
        let payload = identities_answer(&[(b"other", b"YmxvYg==")]);
        let key_blobs = parse_identities_answer(&payload)?;
        assert!(
            !key_blobs
                .iter()
                .any(|key_blob| expected.matches_agent_key_blob(key_blob))
        );
        Ok(())
    }

    #[test]
    fn cursor_take_u32_detects_truncation() {
        let mut cursor = ByteCursor::new(&[0u8, 1, 2]);
        assert!(cursor.take_u32().is_err());
    }
}
