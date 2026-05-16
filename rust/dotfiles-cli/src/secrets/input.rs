//! `dotfiles secrets` の command 入力を storage model へ接続する入力層。
//!
//! 端末 I/O は `util::terminal`、memory lock は `util::protection` に委譲し、この層は
//! prompt / stdin / JSON schema を command の入力契約として固定する。

use anyhow::bail;
use serde::Deserialize;
use zeroize::Zeroizing;

use crate::Result;

use super::{
    storage::{BootstrapSecrets, SecretBytes},
    util::terminal,
};

/// prompt 入力で得た secret を、application の保護境界へ移すまで zeroize 対象にする。
pub(crate) struct SecretInputBuffer {
    secret: SecretBytes,
}

impl From<Zeroizing<Vec<u8>>> for SecretInputBuffer {
    /// prompt 入力の allocation を作り替えず、storage model の secret 所有値へ移す。
    fn from(buffer: Zeroizing<Vec<u8>>) -> Self {
        Self {
            secret: buffer.into(),
        }
    }
}

impl From<SecretInputBuffer> for SecretBytes {
    /// application 層が保護済み値を作る直前に、入力型から storage model へ所有権を移す。
    fn from(input: SecretInputBuffer) -> Self {
        input.secret
    }
}

/// 表示 prompt を使う secret は、端末 newline 契約と byte 上限を入力境界で検証する。
pub(super) fn read_visible_secret_line(prompt: &str, limit: usize) -> Result<SecretInputBuffer> {
    let input =
        terminal::read_visible_line_bytes(prompt, limit, "visible secret input is too large")?;
    Ok(input.into())
}

/// 保存対象 secret の hidden prompt は、PIN と異なる上限エラー契約を持つ。
pub(super) fn read_hidden_secret(prompt: &str, limit: usize) -> Result<SecretInputBuffer> {
    let value =
        terminal::read_hidden_bytes_with_limit(prompt, limit, "hidden secret input is too large")?;
    Ok(value.into())
}

/// YubiKey PIN は PIV session 検証専用で、storage model へ変換しない。
pub(crate) fn read_yubikey_pin() -> Result<Zeroizing<Vec<u8>>> {
    terminal::read_hidden_bytes("YubiKey PIN: ")
}

/// `--stdin-json` は bootstrap schema から外れる key 欠落や型違いを登録前に拒否する。
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
    /// JSON 入力の 3 field を bootstrap 登録用の storage model へ所有権ごと移す。
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

/// 復号後の secret bytes は、TTY 拒否済みの stdout に書き込む。
pub(crate) fn write_secret_to_stdout(bytes: &[u8]) -> Result<()> {
    ensure_secret_stdout_not_terminal()?;
    terminal::write_all_stdout(bytes)
}

/// 実プロセス以外の境界でも TTY 出力拒否の error contract を共有する。
pub(crate) fn reject_secret_stdout_terminal() -> Result<()> {
    bail!("refusing to write secret to terminal; redirect stdout to a file or pipe");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bootstrap_secrets_json_accepts_expected_schema() -> Result<()> {
        let secrets = parse_bootstrap_secrets_json(
            br#"{
                "bw-email": "alice@example.com",
                "bw-password": "password",
                "bws-access-token": "token"
            }"#,
        )?;

        secrets
            .bw_email
            .with_secret(|secret| assert_eq!(secret, b"alice@example.com"));
        secrets
            .bw_password
            .with_secret(|secret| assert_eq!(secret, b"password"));
        secrets
            .bws_access_token
            .with_secret(|secret| assert_eq!(secret, b"token"));
        Ok(())
    }

    #[test]
    fn parse_bootstrap_secrets_json_rejects_missing_field() {
        let result = parse_bootstrap_secrets_json(
            br#"{
                "bw-email": "alice@example.com",
                "bw-password": "password"
            }"#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn parse_bootstrap_secrets_json_rejects_duplicate_field() {
        let result = parse_bootstrap_secrets_json(
            br#"{
                "bw-email": "alice@example.com",
                "bw-email": "bob@example.com",
                "bw-password": "password",
                "bws-access-token": "token"
            }"#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn parse_bootstrap_secrets_json_rejects_non_string_field() {
        let result = parse_bootstrap_secrets_json(
            br#"{
                "bw-email": "alice@example.com",
                "bw-password": 123,
                "bws-access-token": "token"
            }"#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn parse_bootstrap_secrets_json_rejects_trailing_garbage() {
        let result = parse_bootstrap_secrets_json(
            br#"{
                "bw-email": "alice@example.com",
                "bw-password": "password",
                "bws-access-token": "token"
            } trailing"#,
        );

        assert!(result.is_err());
    }
}
