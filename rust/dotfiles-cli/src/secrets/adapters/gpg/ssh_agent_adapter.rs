//! `SshAgentPort` を gpg-agent の SSH key list（`sshcontrol`）と SSH agent socket 観測へ接続する adapter。
//!
//! authentication subkey の keygrip を `${GNUPGHOME:-$HOME/.gnupg}/sshcontrol` へ冪等に登録し、SSH support
//! 利用可否を「SSH agent socket（`S.gpg-agent.ssh`）が解決でき、その socket 経路で authentication subkey の
//! identity を SSH agent protocol で識別できる」状態として観測して domain 値（`SshAgentReadiness`）へ翻訳
//! する。`gpgconf` CLI は使わず、socket は `${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` を優先候補として
//! 確認し、その path が socket でない場合だけ既存環境変数 `SSH_AUTH_SOCK` が指す socket へ fallback する
//! （`config/zsh/env.zsh` の上書き条件と同じ前提）。identity の識別は、解決した socket へ接続して SSH agent
//! protocol（`SSH_AGENTC_REQUEST_IDENTITIES`）で公開鍵 identity を列挙し、その comment が authentication
//! subkey の keygrip（gpg-agent が SSH identity comment へ載せる値）と一致するかで判定する。SSH support 充足
//! の業務判定そのものは domain（`SshAgentReadiness::ensure_ready`）へ残す。

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
        domain::gpg_restore::{Keygrip, SshAgentReadiness},
        ports::gpg::SshAgentPort,
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

    fn inspect_ssh_agent(&mut self, keygrip: &Keygrip) -> Result<SshAgentReadiness> {
        // 固定 path が socket ならそれ、socket でなければ既存 `SSH_AUTH_SOCK` が socket ならそれを使う。
        let socket = resolve_ssh_agent_socket();
        let socket_resolved = socket.is_some();
        // identity の識別は、解決した socket へ接続して SSH agent protocol で公開鍵 identity を列挙し、
        // その comment が authentication subkey の keygrip と一致するかで判定する。socket が解決できない、
        // または接続/列挙に失敗した場合は識別不能（false）として停止条件を弱めない。
        let authentication_identity_present = match socket {
            Some(path) => agent_identifies_keygrip(&path, keygrip).unwrap_or(false),
            None => false,
        };
        Ok(SshAgentReadiness {
            socket_resolved,
            authentication_identity_present,
        })
    }
}

/// SSH agent socket を解決する。
///
/// 設計「zsh 環境変数決定」と `config/zsh/env.zsh` の上書き条件に合わせ、`${GNUPGHOME:-$HOME/.gnupg}/
/// S.gpg-agent.ssh` が socket ならそれを優先し、socket でない場合だけ既存環境変数 `SSH_AUTH_SOCK` が
/// 指す path が socket ならそれへ fallback する。`gpgconf` CLI は使わない。
fn resolve_ssh_agent_socket() -> Option<PathBuf> {
    if let Ok(Some(fixed)) = ssh_agent_socket_path()
        && is_socket(&fixed)
    {
        return Some(fixed);
    }
    if let Some(env) = std::env::var_os("SSH_AUTH_SOCK") {
        let env = PathBuf::from(env);
        if is_socket(&env) {
            return Some(env);
        }
    }
    None
}

/// SSH agent protocol で identity を列挙し、対象 keygrip を comment に持つ identity が存在するかを返す。
///
/// 解決済み socket へ接続し、`SSH_AGENTC_REQUEST_IDENTITIES` を送って `SSH_AGENT_IDENTITIES_ANSWER` を
/// 解析する。gpg-agent は sshcontrol 由来 identity の comment へ keygrip（uppercase hex 40）を載せるため、
/// comment に対象 keygrip が含まれる identity を「authentication subkey が SSH identity として識別可能」と
/// 判定する。接続/送受信/解析に失敗した場合は、socket への接続可否を最低限の identity 観測代替とせず、
/// 停止条件を弱めないため `Err` を返して呼び出し側で false に倒す。
#[cfg(unix)]
fn agent_identifies_keygrip(socket: &Path, keygrip: &Keygrip) -> Result<bool> {
    let comments = request_ssh_identities(socket)?;
    Ok(comments
        .iter()
        .any(|comment| comment_contains_keygrip(comment, keygrip)))
}

#[cfg(not(unix))]
fn agent_identifies_keygrip(_socket: &Path, _keygrip: &Keygrip) -> Result<bool> {
    Ok(false)
}

/// SSH identity の comment が対象 keygrip を識別できるかを照合する。
///
/// gpg-agent は comment へ keygrip を載せるため、空白区切りトークンのいずれかが keygrip と
/// 大文字小文字を無視して一致する場合に識別可能とみなす。
fn comment_contains_keygrip(comment: &str, keygrip: &Keygrip) -> bool {
    comment
        .split_whitespace()
        .any(|token| token.eq_ignore_ascii_case(keygrip.as_str()))
}

/// SSH agent protocol の message 種別（必要な値のみ）。
#[cfg(unix)]
const SSH_AGENTC_REQUEST_IDENTITIES: u8 = 11;
#[cfg(unix)]
const SSH_AGENT_IDENTITIES_ANSWER: u8 = 12;

/// 解決済み socket へ接続し、列挙された identity の comment 文字列を返す。
///
/// SSH agent protocol は length-prefixed frame（先頭 4 byte の big-endian length に payload が続く）で、
/// `SSH_AGENT_IDENTITIES_ANSWER` の payload は `count`（u32）に続いて `(key_blob string, comment string)`
/// が count 回並ぶ。各 string は 4 byte 長 prefix 付きである。secret material は要求・受信しない。
#[cfg(unix)]
fn request_ssh_identities(socket: &Path) -> Result<Vec<String>> {
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

/// `SSH_AGENT_IDENTITIES_ANSWER` payload を解析し、各 identity の comment を返す。
#[cfg(unix)]
fn parse_identities_answer(payload: &[u8]) -> Result<Vec<String>> {
    let mut cursor = ByteCursor::new(payload);
    let message_type = cursor.take_u8()?;
    if message_type != SSH_AGENT_IDENTITIES_ANSWER {
        anyhow::bail!("unexpected SSH agent response message type");
    }
    let count = cursor.take_u32()?;
    let mut comments = Vec::with_capacity(count as usize);
    for _ in 0..count {
        // key blob は識別に使わないため読み飛ばし、comment 文字列だけを取り出す。
        let _key_blob = cursor.take_string()?;
        let comment = cursor.take_string()?;
        comments.push(String::from_utf8_lossy(comment).into_owned());
    }
    Ok(comments)
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

/// `${GNUPGHOME:-$HOME/.gnupg}` を解決する。
fn gnupg_home() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("GNUPGHOME") {
        return Ok(PathBuf::from(home));
    }
    let home = std::env::var_os("HOME").context("HOME is not set; cannot resolve GnuPG home")?;
    Ok(PathBuf::from(home).join(".gnupg"))
}

/// gpg-agent の SSH key list（`sshcontrol`）の path を返す。
fn sshcontrol_path() -> Result<PathBuf> {
    Ok(gnupg_home()?.join("sshcontrol"))
}

/// `${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` を SSH agent socket の優先候補として返す。
fn ssh_agent_socket_path() -> Result<Option<PathBuf>> {
    Ok(Some(gnupg_home()?.join("S.gpg-agent.ssh")))
}

/// 指定 path が socket として存在するかを返す。
fn is_socket(path: &PathBuf) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        std::fs::metadata(path)
            .map(|metadata| metadata.file_type().is_socket())
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

/// `sshcontrol` に keygrip（uppercase hex 40）が既に登録されているかを返す。
fn sshcontrol_contains(path: &PathBuf, keygrip: &Keygrip) -> Result<bool> {
    let file = match OpenOptions::new().read(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(anyhow::Error::new(error).context("failed to read gpg-agent sshcontrol"));
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
    //! SSH agent protocol の identity 応答解析と keygrip 識別という adapter 翻訳ロジックの単体テスト。
    //!
    //! socket 接続を伴わない純粋な byte decode と comment 照合だけを検証し、外部 agent は呼ばない。

    use super::{
        ByteCursor, SSH_AGENT_IDENTITIES_ANSWER, comment_contains_keygrip, parse_identities_answer,
    };
    use crate::secrets::domain::gpg_restore::Keygrip;

    const KEYGRIP: &str = "0123456789ABCDEF0123456789ABCDEF01234567";

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
    fn parses_identity_comments_in_order() -> crate::Result<()> {
        let payload = identities_answer(&[(b"blob-a", b"comment-a"), (b"blob-b", b"comment-b")]);
        let comments = parse_identities_answer(&payload)?;
        assert_eq!(
            comments,
            vec!["comment-a".to_owned(), "comment-b".to_owned()]
        );
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
    fn matches_keygrip_in_comment_case_insensitively() -> crate::Result<()> {
        let keygrip = Keygrip::parse(KEYGRIP)?;
        // gpg-agent は comment に keygrip を載せる。大文字小文字を無視して識別する。
        assert!(comment_contains_keygrip(&KEYGRIP.to_lowercase(), &keygrip));
        // keygrip が空白区切りトークンとして現れる comment（例: keygrip にラベルが続く）も識別する。
        assert!(comment_contains_keygrip(
            &format!("{KEYGRIP} cardno:0006"),
            &keygrip
        ));
        // keygrip を部分文字列として含むだけの別トークンは識別しない（false positive を作らない）。
        assert!(!comment_contains_keygrip(
            &format!("ssh:{KEYGRIP}"),
            &keygrip
        ));
        assert!(!comment_contains_keygrip("unrelated identity", &keygrip));
        Ok(())
    }

    #[test]
    fn cursor_take_u32_detects_truncation() {
        let mut cursor = ByteCursor::new(&[0u8, 1, 2]);
        assert!(cursor.take_u32().is_err());
    }
}
