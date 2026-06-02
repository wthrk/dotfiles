//! 端末/標準入力の secret I/O を process helper と port 契約の間で翻訳する adapter。
//!
//! prompt 文言や入力上限はこの境界に閉じ、use case 手順や storage 判定は扱わない。

use std::collections::BTreeMap;

use crate::{
    Result,
    secrets::{
        domain::{gpg_restore::OpenSshPublicKey, manifest::BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT},
        ports::io::{
            BackupUpdateConfirmationPort, BootstrapSecretDocumentInputPort, ClockPort,
            PasswordStoreRemoteInputPort, PinInputPort, ProvisioningAccessTokenInputPort,
            RotationContinuationPort, SecretInputPort, SecretOutputPort, SshPublicKeyOutputPort,
        },
        support::{
            clock, process_io,
            protection::{ProtectedSecret, write_secret_stdout},
        },
    },
};

/// process-generic helper と secret I/O port の間で保護値変換を集約する内部 adapter。
///
/// この型は `ProcessIoAdapter` の内部委譲先であり、prompt/stdin/stdout の各 port 実装が
/// 同じ protection 変換境界を使うために存在する。use case 手順や secret の業務意味は持たない。
#[derive(Default)]
struct RealSecretIoAdapter;

impl PinInputPort for RealSecretIoAdapter {
    fn read_pin(&self) -> Result<ProtectedSecret> {
        process_io::read_hidden_line(
            "YubiKey PIN: ",
            crate::secrets::domain::piv::PIV_PIN_MAX_LEN,
            "YubiKey PIN is too long",
        )
    }
}

impl SecretInputPort for RealSecretIoAdapter {
    fn read_bw_email_secret(&self) -> Result<ProtectedSecret> {
        process_io::read_visible_line("bw-email: ", 16 * 1024, "visible secret input is too large")
    }

    fn read_bw_password_secret(&self) -> Result<ProtectedSecret> {
        process_io::read_hidden_line(
            "bw-password: ",
            16 * 1024,
            "hidden secret input is too large",
        )
    }

    fn read_bws_access_token_secret(&self) -> Result<ProtectedSecret> {
        process_io::read_hidden_line(
            "bws-access-token: ",
            16 * 1024,
            "hidden secret input is too large",
        )
    }

    fn read_streamed_secret(&self) -> Result<ProtectedSecret> {
        process_io::read_stdin_line(16 * 1024, "stdin secret input is too large")
    }
}

impl ProvisioningAccessTokenInputPort for RealSecretIoAdapter {
    fn read_provisioning_access_token(&self) -> Result<ProtectedSecret> {
        // provisioning 用 access token は書込み可能な実 credential のため secret として扱い、
        // stdin が terminal のとき hidden prompt（raw mode・echo なし）、非 terminal（pipe）のとき
        // stdin 1 行を保護 buffer へ読む。いずれも平文を argv / ログ / 端末表示へ残さない。
        const MAX_LEN: usize = 16 * 1024;
        const TOO_LONG_MESSAGE: &str = "provisioning access token input is too large";
        if process_io::stdin_is_terminal() {
            process_io::read_hidden_line("provisioning-access-token: ", MAX_LEN, TOO_LONG_MESSAGE)
        } else {
            process_io::read_stdin_line(MAX_LEN, TOO_LONG_MESSAGE)
        }
    }
}

impl PasswordStoreRemoteInputPort for RealSecretIoAdapter {
    fn read_password_store_remote_url(&self) -> Result<String> {
        // clone URL は秘密情報ではないため保護 buffer・非表示入力を使わず、可視入力 / pipe で読む。
        // stdin が terminal のとき可視プロンプト（エコーする通常入力）、非 terminal（pipe）のとき stdin 1 行。
        const MAX_LEN: usize = 16 * 1024;
        const TOO_LONG_MESSAGE: &str = "password-store-remote input is too large";
        if process_io::stdin_is_terminal() {
            process_io::read_visible_plain_line(
                "password-store-remote: ",
                MAX_LEN,
                TOO_LONG_MESSAGE,
            )
        } else {
            process_io::read_stdin_plain_line(MAX_LEN, TOO_LONG_MESSAGE)
        }
    }
}

impl RotationContinuationPort for RealSecretIoAdapter {
    fn continue_rotation(&self) -> Result<bool> {
        if !process_io::stdin_is_terminal() {
            return Ok(false);
        }
        let answer = process_io::read_control_line("rotate another YubiKey? [y/N]: ")?;
        Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "YES" | "Yes"))
    }
}

impl BootstrapSecretDocumentInputPort for RealSecretIoAdapter {
    fn read_bootstrap_secret_fields(&self) -> Result<BTreeMap<String, ProtectedSecret>> {
        let protected =
            process_io::read_stdin_all(64 * 1024, "bootstrap secret JSON input is too large")?;
        protected.decode_json_string_map(BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT)
    }
}

impl SecretOutputPort for RealSecretIoAdapter {
    fn write_secret(&self, secret: &ProtectedSecret) -> Result<()> {
        write_secret_stdout(secret)
    }
}

impl SshPublicKeyOutputPort for RealSecretIoAdapter {
    fn write_ssh_public_key(&self, public_key: &OpenSshPublicKey) -> Result<()> {
        // 公開鍵は秘密情報ではないため terminal でも 1 行で出力する。
        println!("{}", public_key.as_str());
        Ok(())
    }
}

impl ClockPort for RealSecretIoAdapter {
    fn now_rfc3339_utc(&self) -> Result<String> {
        clock::now_rfc3339_utc()
    }
}

impl BackupUpdateConfirmationPort for RealSecretIoAdapter {
    fn confirm_backup_update(
        &self,
        project_name: &str,
        secret_name: &str,
        primary_fingerprint: &str,
        assume_overwrite: bool,
    ) -> Result<bool> {
        if !process_io::stdin_is_terminal() {
            // 非対話実行では明示的上書き許可 option がある場合だけ更新を許可する。
            return Ok(assume_overwrite);
        }
        let prompt = format!(
            "update BWS secret {secret_name} in project {project_name} \
             (primary fingerprint {primary_fingerprint})? [y/N]: "
        );
        let answer = process_io::read_control_line(&prompt)?;
        Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "YES" | "Yes"))
    }

    fn confirm_secret_overwrite(
        &self,
        project_name: &str,
        secret_name: &str,
        assume_overwrite: bool,
    ) -> Result<bool> {
        if !process_io::stdin_is_terminal() {
            // 非対話実行では明示的上書き許可 option がある場合だけ更新を許可する。
            return Ok(assume_overwrite);
        }
        let prompt = format!("update BWS secret {secret_name} in project {project_name}? [y/N]: ");
        let answer = process_io::read_control_line(&prompt)?;
        Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "YES" | "Yes"))
    }
}

/// process/terminal I/O helper を secret 入出力 port 群へ翻訳する adapter。
///
/// caller は必要な入力・出力 capability だけを呼ぶ。adapter は prompt/stdin/stdout の技術制約を
/// 吸収し、use case の順序や secret の業務意味を決めない。
#[derive(Default)]
pub(super) struct ProcessIoAdapter {
    secret_io: RealSecretIoAdapter,
}

impl PinInputPort for ProcessIoAdapter {
    fn read_pin(&self) -> Result<ProtectedSecret> {
        self.secret_io.read_pin()
    }
}

impl SecretInputPort for ProcessIoAdapter {
    fn read_bw_email_secret(&self) -> Result<ProtectedSecret> {
        self.secret_io.read_bw_email_secret()
    }

    fn read_bw_password_secret(&self) -> Result<ProtectedSecret> {
        self.secret_io.read_bw_password_secret()
    }

    fn read_bws_access_token_secret(&self) -> Result<ProtectedSecret> {
        self.secret_io.read_bws_access_token_secret()
    }

    fn read_streamed_secret(&self) -> Result<ProtectedSecret> {
        self.secret_io.read_streamed_secret()
    }
}

impl ProvisioningAccessTokenInputPort for ProcessIoAdapter {
    fn read_provisioning_access_token(&self) -> Result<ProtectedSecret> {
        self.secret_io.read_provisioning_access_token()
    }
}

impl PasswordStoreRemoteInputPort for ProcessIoAdapter {
    fn read_password_store_remote_url(&self) -> Result<String> {
        self.secret_io.read_password_store_remote_url()
    }
}

impl RotationContinuationPort for ProcessIoAdapter {
    fn continue_rotation(&self) -> Result<bool> {
        self.secret_io.continue_rotation()
    }
}

impl BootstrapSecretDocumentInputPort for ProcessIoAdapter {
    fn read_bootstrap_secret_fields(&self) -> Result<BTreeMap<String, ProtectedSecret>> {
        self.secret_io.read_bootstrap_secret_fields()
    }
}

impl SecretOutputPort for ProcessIoAdapter {
    fn write_secret(&self, secret: &ProtectedSecret) -> Result<()> {
        self.secret_io.write_secret(secret)
    }
}

impl SshPublicKeyOutputPort for ProcessIoAdapter {
    fn write_ssh_public_key(&self, public_key: &OpenSshPublicKey) -> Result<()> {
        self.secret_io.write_ssh_public_key(public_key)
    }
}

impl ClockPort for ProcessIoAdapter {
    fn now_rfc3339_utc(&self) -> Result<String> {
        self.secret_io.now_rfc3339_utc()
    }
}

impl BackupUpdateConfirmationPort for ProcessIoAdapter {
    fn confirm_backup_update(
        &self,
        project_name: &str,
        secret_name: &str,
        primary_fingerprint: &str,
        assume_overwrite: bool,
    ) -> Result<bool> {
        self.secret_io.confirm_backup_update(
            project_name,
            secret_name,
            primary_fingerprint,
            assume_overwrite,
        )
    }

    fn confirm_secret_overwrite(
        &self,
        project_name: &str,
        secret_name: &str,
        assume_overwrite: bool,
    ) -> Result<bool> {
        self.secret_io
            .confirm_secret_overwrite(project_name, secret_name, assume_overwrite)
    }
}
