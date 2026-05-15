//! `dotfiles secrets` の command 入力を storage へ渡せる値に変換する。
//!
//! prompt と stdin の読み取り方法はこの module で選ぶ。端末の低水準操作は `util`、
//! memory lock と zeroize 対象 buffer の所有は `util::protection` が扱う。

use anyhow::bail;
use serde::Deserialize;
use std::io;
use zeroize::Zeroizing;

use crate::Result;

use super::{
    storage::{BootstrapSecrets, SecretBytes},
    util::{
        protection::{ProtectedInputBuffer, SecretMemoryGuard},
        terminal,
    },
};

/// prompt/stdin/JSON field から受け取った secret を、保護対象に渡す直前まで保持する入力型。
pub(crate) struct SecretInputBuffer {
    secret: SecretBytes,
}

impl From<Zeroizing<Vec<u8>>> for SecretInputBuffer {
    /// prompt 入力は zeroize 対象 buffer の所有権を維持したまま storage secret 型へ移す。
    fn from(buffer: Zeroizing<Vec<u8>>) -> Self {
        Self {
            secret: buffer.into(),
        }
    }
}

impl From<SecretInputBuffer> for SecretBytes {
    /// 入力境界で確定した secret は、memory lock 直前に storage model へ移す。
    fn from(input: SecretInputBuffer) -> Self {
        input.secret
    }
}

impl From<ProtectedInputBuffer> for SecretInputBuffer {
    /// `--stdin` の単一 secret は行区切りの末尾 newline だけを除いて storage secret 型へ移す。
    fn from(buffer: ProtectedInputBuffer) -> Self {
        Self {
            secret: buffer.into_trimmed_bytes().into(),
        }
    }
}

/// 表示入力を許す secret は、読み取り直後に入力用 secret 型へ移す。
pub(super) fn read_visible_secret_line(prompt: &str, limit: usize) -> Result<SecretInputBuffer> {
    let input =
        terminal::read_visible_line_bytes(prompt, limit, "visible secret input is too large")?;
    Ok(input.into())
}

/// 保存対象 secret の hidden prompt は PIN 入力と型を分ける。
pub(super) fn read_hidden_secret(prompt: &str) -> Result<SecretInputBuffer> {
    let value = terminal::read_hidden_bytes(prompt)?;
    Ok(value.into())
}

/// YubiKey PIN は PIV session 検証だけに使い、storage secret にはしない。
pub(crate) fn read_yubikey_pin() -> Result<Zeroizing<Vec<u8>>> {
    terminal::read_hidden_bytes("YubiKey PIN: ")
}

/// `--stdin` の単一 secret は行区切りの末尾 newline だけを除いて保存する。
pub(super) fn read_one_stdin_secret(
    limit: usize,
    memory: Option<&SecretMemoryGuard>,
) -> Result<SecretInputBuffer> {
    let input = ProtectedInputBuffer::read_from(
        io::stdin(),
        limit,
        "stdin secret input is too large",
        memory,
    )?;
    Ok(input.into())
}

/// `--stdin-json` は既定 schema 以外の key 欠落や型違いを serde error として拒否する。
pub(super) fn parse_bootstrap_secrets_json(input: &[u8]) -> Result<BootstrapSecrets> {
    let input: BootstrapSecretsInput = serde_json::from_slice(input)?;
    Ok(input.into_bootstrap_secrets())
}

#[derive(Deserialize)]
struct BootstrapSecretsInput {
    #[serde(rename = "bw-email")]
    bw_email: SecretBytes,
    #[serde(rename = "bw-password")]
    bw_password: SecretBytes,
    #[serde(rename = "bws-access-token")]
    bws_access_token: SecretBytes,
}

impl BootstrapSecretsInput {
    /// serde が検証した 3 field を、bootstrap 登録用の storage model へ移す。
    fn into_bootstrap_secrets(self) -> BootstrapSecrets {
        BootstrapSecrets {
            bw_email: self.bw_email,
            bw_password: self.bw_password,
            bws_access_token: self.bws_access_token,
        }
    }
}

/// 低水準 `get` の出力先が TTY の場合は平文 secret を書かない。
pub(crate) fn ensure_secret_stdout_not_terminal() -> Result<()> {
    if terminal::stdout_is_terminal() {
        reject_secret_stdout_terminal()?;
    }
    Ok(())
}

/// 呼び出し側が出力先を復号前に確認したうえで、secret bytes を stdout へ渡す。
pub(crate) fn write_secret_to_stdout(bytes: &[u8]) -> Result<()> {
    ensure_secret_stdout_not_terminal()?;
    terminal::write_all_stdout(bytes)
}

/// 実プロセス以外の境界でも TTY 出力拒否の error contract を共有する。
pub(crate) fn reject_secret_stdout_terminal() -> Result<()> {
    bail!("refusing to write secret to terminal; redirect stdout to a file or pipe");
}
