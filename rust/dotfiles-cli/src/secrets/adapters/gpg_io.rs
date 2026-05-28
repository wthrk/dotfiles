//! restore-gpg / export-ssh-public-key を GPG 実プロセス境界へ接続する adapter。

use std::{env, io::Write, process::Command};

use anyhow::{Context, bail};

use crate::{
    Result,
    secrets::{domain::material::SecretMaterial, ports},
};

const GPG_SECRET_KEY_BACKUP_ENV: &str = "DOTFILES_SECRETS_GPG_SECRET_KEY_BACKUP";

/// GPG/SSH 操作を process 呼び出しで実行する adapter。
#[derive(Default)]
pub(super) struct GpgRecoveryAdapter;

struct SecretKeyCapabilities {
    has_encryption: bool,
    has_signing: bool,
    has_authentication: bool,
    authentication_fingerprint: Option<String>,
}

impl ports::GpgRecoveryPort for GpgRecoveryAdapter {
    fn read_gpg_secret_key_backup(&self, bws_access_token: &SecretMaterial) -> Result<String> {
        if bws_access_token.len() == 0 {
            bail!("bws-access-token must not be empty");
        }

        let backup = env::var(GPG_SECRET_KEY_BACKUP_ENV).with_context(|| {
            format!(
                "Bitwarden Secrets Manager integration is unavailable; set {GPG_SECRET_KEY_BACKUP_ENV} for local recovery"
            )
        })?;
        if backup.trim().is_empty() {
            bail!("{GPG_SECRET_KEY_BACKUP_ENV} must not be empty");
        }
        Ok(backup)
    }

    fn import_gpg_secret_key(&self, armored_secret_key: &str) -> Result<()> {
        if armored_secret_key.trim().is_empty() {
            bail!("gpg secret key backup must not be empty");
        }
        let mut child = Command::new("gpg")
            .args(["--batch", "--import"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("failed to start gpg --import")?;
        {
            let Some(stdin) = child.stdin.as_mut() else {
                bail!("failed to open gpg import stdin");
            };
            stdin
                .write_all(armored_secret_key.as_bytes())
                .context("failed to write gpg secret key backup to gpg --import stdin")?;
        }
        let output = child
            .wait_with_output()
            .context("failed to wait gpg --import")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("gpg secret key import failed: {stderr}");
        }
        Ok(())
    }

    fn verify_gpg_restore_prerequisites(&self) -> Result<()> {
        let capabilities = self.collect_secret_key_capabilities()?;
        if !capabilities.has_encryption {
            bail!("imported GPG secret key does not have encryption subkey");
        }
        if !capabilities.has_signing {
            bail!("imported GPG secret key does not have signing subkey");
        }
        if !capabilities.has_authentication {
            bail!("imported GPG secret key does not have authentication subkey");
        }

        let output = Command::new("gpg-connect-agent")
            .args(["GETINFO", "ssh_socket_name", "/bye"])
            .output()
            .context("failed to run gpg-connect-agent")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("gpg-agent SSH support is unavailable: {stderr}");
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.lines().any(|line| {
            line.strip_prefix("D ")
                .is_some_and(|socket| !socket.trim().is_empty())
        }) {
            bail!("gpg-agent SSH support is unavailable");
        }
        Ok(())
    }

    fn export_ssh_public_key(&self) -> Result<String> {
        let capabilities = self.collect_secret_key_capabilities()?;
        let fingerprint = capabilities
            .authentication_fingerprint
            .ok_or_else(|| anyhow::anyhow!("authentication subkey fingerprint is missing"))?;
        let output = Command::new("gpg")
            .args(["--batch", "--export-ssh-key", &fingerprint])
            .output()
            .context("failed to run gpg --export-ssh-key")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("failed to export SSH public key from GPG authentication subkey: {stderr}");
        }
        let public_key = String::from_utf8(output.stdout).context("SSH public key is not UTF-8")?;
        let public_key = public_key.trim().to_string();
        if public_key.is_empty() {
            bail!("gpg --export-ssh-key returned empty output");
        }
        Ok(public_key)
    }
}

impl ports::SshPublicKeyOutputPort for GpgRecoveryAdapter {
    fn write_ssh_public_key(&self, public_key: &str) -> Result<()> {
        println!("{public_key}");
        Ok(())
    }
}

impl GpgRecoveryAdapter {
    /// `gpg --with-colons --list-secret-keys` 出力から subkey capability を抽出する。
    ///
    /// `sec`/`ssb` の capability フィールドを走査し、authentication capability を持つ鍵の
    /// fingerprint を `fpr` レコードから 1 件取得する。
    fn collect_secret_key_capabilities(&self) -> Result<SecretKeyCapabilities> {
        let output = Command::new("gpg")
            .args(["--batch", "--with-colons", "--list-secret-keys"])
            .output()
            .context("failed to run gpg --list-secret-keys")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("failed to inspect gpg secret keys: {stderr}");
        }
        let stdout = String::from_utf8(output.stdout)
            .context("gpg --list-secret-keys output is not UTF-8")?;
        let mut capabilities = SecretKeyCapabilities {
            has_encryption: false,
            has_signing: false,
            has_authentication: false,
            authentication_fingerprint: None,
        };
        let mut pending_auth_fingerprint = false;
        for line in stdout.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            let Some(record_type) = fields.first().copied() else {
                continue;
            };
            match record_type {
                "sec" | "ssb" => {
                    let caps = fields
                        .get(11)
                        .map(|value| value.to_ascii_lowercase())
                        .unwrap_or_default();
                    if caps.contains('e') {
                        capabilities.has_encryption = true;
                    }
                    if caps.contains('s') {
                        capabilities.has_signing = true;
                    }
                    if caps.contains('a') {
                        capabilities.has_authentication = true;
                        pending_auth_fingerprint = true;
                    } else {
                        pending_auth_fingerprint = false;
                    }
                }
                "fpr"
                    if pending_auth_fingerprint
                        && capabilities.authentication_fingerprint.is_none() =>
                {
                    if let Some(fingerprint) = fields.get(9).copied() {
                        if !fingerprint.is_empty() {
                            capabilities.authentication_fingerprint = Some(fingerprint.to_string());
                        }
                    }
                    pending_auth_fingerprint = false;
                }
                _ => {}
            }
        }
        Ok(capabilities)
    }
}
