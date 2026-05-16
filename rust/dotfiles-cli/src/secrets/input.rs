//! `dotfiles secrets` の利用者入力を storage model へ変換する入力層。
//!
//! 端末 I/O は `util::terminal`、memory lock は `util::protection` に委譲し、この層は
//! prompt、stdin、JSON schema の入力形式と error contract を固定する。

use anyhow::bail;
use serde::Deserialize;
use zeroize::Zeroizing;

use crate::Result;

use super::{
    storage::{BootstrapSecrets, SecretBytes},
    util::terminal,
};

/// prompt 入力で得た byte 列を、application の保護境界へ移すまで zeroize 対象として保持する。
pub(crate) struct SecretInputBuffer {
    secret: SecretBytes,
}

impl From<Zeroizing<Vec<u8>>> for SecretInputBuffer {
    /// terminal adapter が返した allocation を作り替えず、storage model の所有値へ移す。
    fn from(buffer: Zeroizing<Vec<u8>>) -> Self {
        Self {
            secret: buffer.into(),
        }
    }
}

impl From<SecretInputBuffer> for SecretBytes {
    /// application 層が保護済み値を作る直前に、入力 buffer の所有権を storage model へ移す。
    fn from(input: SecretInputBuffer) -> Self {
        input.secret
    }
}

/// 表示 prompt で 1 行を読み、端末 newline の除去と byte 上限を入力境界で検証する。
pub(super) fn read_visible_secret_line(prompt: &str, limit: usize) -> Result<SecretInputBuffer> {
    let input =
        terminal::read_visible_line_bytes(prompt, limit, "visible secret input is too large")?;
    Ok(input.into())
}

/// echo なしの prompt で 1 行を読み、secret 入力用の byte 上限 error を適用する。
pub(super) fn read_hidden_secret(prompt: &str, limit: usize) -> Result<SecretInputBuffer> {
    let value =
        terminal::read_hidden_bytes_with_limit(prompt, limit, "hidden secret input is too large")?;
    Ok(value.into())
}

/// echo なしの prompt で YubiKey PIN を読み、PIV session 検証用の byte buffer として返す。
pub(crate) fn read_yubikey_pin() -> Result<Zeroizing<Vec<u8>>> {
    terminal::read_hidden_bytes("YubiKey PIN: ")
}

/// stdin の JSON bytes を bootstrap 登録用 schema として parse し、型付き model へ変換する。
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
    /// JSON 入力の 3 field を bootstrap 登録用 model へ所有権ごと移す。
    fn into_bootstrap_secrets(self) -> BootstrapSecrets {
        BootstrapSecrets {
            bw_email: self.bw_email,
            bw_password: self.bw_password,
            bws_access_token: self.bws_access_token,
        }
    }
}

/// stdout が TTY の場合は、復号結果を書き込む前に利用者向け error で停止する。
pub(crate) fn ensure_secret_stdout_not_terminal() -> Result<()> {
    if terminal::stdout_is_terminal() {
        reject_secret_stdout_terminal()?;
    }
    Ok(())
}

/// stdout の TTY 拒否を確認してから、復号済み bytes を stdout へ書き込む。
pub(crate) fn write_secret_to_stdout(bytes: &[u8]) -> Result<()> {
    ensure_secret_stdout_not_terminal()?;
    terminal::write_all_stdout(bytes)
}

/// stdout が TTY の場合に返す利用者向け error を、実プロセス境界と test 境界で共有する。
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
