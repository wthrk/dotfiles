//! 端末 secret I/O を process helper と port 契約の間で翻訳する adapter。
//!
//! prompt 文言や入力上限はこの境界に閉じ、use case 手順や storage 判定は扱わない。

use crate::{
    Result,
    secrets::{
        domain::gpg_restore::OpenSshPublicKey,
        ports::io::{
            PasswordStoreRemoteInputPort, PinInputPort, SecretInputPort, SecretOutputPort,
            SshPublicKeyOutputPort,
        },
        support::{
            process_io,
            protection::{ProtectedSecret, write_secret_stdout},
        },
    },
};

/// process-generic helper と secret I/O port の間で保護値変換を集約する内部 adapter。
///
/// この型は `ProcessIoAdapter` の内部委譲先であり、prompt/stdout の各 port 実装が
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
    fn read_bitwarden_client_id_secret(&self) -> Result<ProtectedSecret> {
        process_io::read_hidden_line(
            "bitwarden-client-id: ",
            16 * 1024,
            "hidden secret input is too large",
        )
    }

    fn read_bitwarden_client_secret(&self) -> Result<ProtectedSecret> {
        process_io::read_hidden_line(
            "bitwarden-client-secret: ",
            16 * 1024,
            "hidden secret input is too large",
        )
    }

    fn read_bitwarden_master_password(&self) -> Result<ProtectedSecret> {
        process_io::read_hidden_line(
            "Bitwarden master password: ",
            16 * 1024,
            "hidden secret input is too large",
        )
    }
}

impl PasswordStoreRemoteInputPort for RealSecretIoAdapter {
    fn read_password_store_remote_url(&self) -> Result<String> {
        // clone URL は秘密情報ではないため保護 buffer・非表示入力を使わず、controlling TTY の可視入力で読む。
        // script / 非対話経路が stdin pipe で URL を中継しないよう、stdin 状態に関わらず対話入力だけを許可する。
        // 設定済み password-store の origin 観測は password-store port 側の責務である。
        const MAX_LEN: usize = 16 * 1024;
        const TOO_LONG_MESSAGE: &str = "password-store-remote input is too large";
        process_io::read_visible_plain_line("password-store-remote: ", MAX_LEN, TOO_LONG_MESSAGE)
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

/// process/terminal I/O helper を secret 入出力 port 群へ翻訳する adapter。
///
/// caller は必要な入力・出力 capability だけを呼ぶ。adapter は prompt/stdout の技術制約を
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
    fn read_bitwarden_client_id_secret(&self) -> Result<ProtectedSecret> {
        self.secret_io.read_bitwarden_client_id_secret()
    }

    fn read_bitwarden_client_secret(&self) -> Result<ProtectedSecret> {
        self.secret_io.read_bitwarden_client_secret()
    }
    fn read_bitwarden_master_password(&self) -> Result<ProtectedSecret> {
        self.secret_io.read_bitwarden_master_password()
    }
}

impl PasswordStoreRemoteInputPort for ProcessIoAdapter {
    fn read_password_store_remote_url(&self) -> Result<String> {
        self.secret_io.read_password_store_remote_url()
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
