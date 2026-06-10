//! 端末/標準入力の secret I/O を process helper と port 契約の間で翻訳する adapter。
//!
//! prompt 文言や入力上限はこの境界に閉じ、use case 手順や storage 判定は扱わない。

use crate::{
    Result,
    secrets::{
        domain::gpg_restore::OpenSshPublicKey,
        ports::io::{
            BwOtpInputPort, BwsAccessTokenInputPort, PasswordStoreRemoteInputPort, PinInputPort,
            SecretInputPort, SecretOutputPort, SshPublicKeyOutputPort,
        },
        support::{
            process_io,
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

impl BwsAccessTokenInputPort for RealSecretIoAdapter {
    fn read_bws_access_token_for_provisioning(&self) -> Result<ProtectedSecret> {
        // BWS access token は BWS 操作に使う実 credential のため secret として扱い、
        // stdin が terminal のとき hidden prompt（raw mode・echo なし）、非 terminal（pipe）のとき
        // stdin 1 行を保護 buffer へ読む。いずれも平文を argv / ログ / 端末表示へ残さない。
        const MAX_LEN: usize = 16 * 1024;
        const TOO_LONG_MESSAGE: &str = "bws access token input is too large";
        if process_io::stdin_is_terminal() {
            process_io::read_hidden_line(
                "bws-access-token (create/use): ",
                MAX_LEN,
                TOO_LONG_MESSAGE,
            )
        } else {
            process_io::read_stdin_line(MAX_LEN, TOO_LONG_MESSAGE)
        }
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

impl BwOtpInputPort for RealSecretIoAdapter {
    fn read_bw_otp(&self) -> Result<String> {
        // YubiKey OTP は touch 生成・単回利用で `bw login --code <otp>` の argv に載る前提（spec L178）のため、
        // 保護 buffer・非表示入力を使わず可視入力 / pipe で読む。stdin が terminal のとき可視プロンプト
        // （エコーする通常入力）、非 terminal（pipe）のとき stdin 1 行。OTP 妥当性判断は domain rule に委ねる。
        const MAX_LEN: usize = 1024;
        const TOO_LONG_MESSAGE: &str = "YubiKey OTP input is too large";
        if process_io::stdin_is_terminal() {
            process_io::read_visible_plain_line("yubikey-otp: ", MAX_LEN, TOO_LONG_MESSAGE)
        } else {
            process_io::read_stdin_plain_line(MAX_LEN, TOO_LONG_MESSAGE)
        }
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

impl BwsAccessTokenInputPort for ProcessIoAdapter {
    fn read_bws_access_token_for_provisioning(&self) -> Result<ProtectedSecret> {
        self.secret_io.read_bws_access_token_for_provisioning()
    }
}

impl PasswordStoreRemoteInputPort for ProcessIoAdapter {
    fn read_password_store_remote_url(&self) -> Result<String> {
        self.secret_io.read_password_store_remote_url()
    }
}

impl BwOtpInputPort for ProcessIoAdapter {
    fn read_bw_otp(&self) -> Result<String> {
        self.secret_io.read_bw_otp()
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
