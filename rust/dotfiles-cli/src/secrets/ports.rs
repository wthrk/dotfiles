//! `dotfiles secrets` application 層が外部境界へ要求する port。
//!
//! application はこの module の trait だけに依存し、実機 YubiKey と test stub の具体的な
//! 入出力差分は adapter 側に閉じる。

use crate::Result;

use super::domain::PivObjectId;
use super::support::protection::{ProtectedSecret, SecretSession};

/// application use case が利用する外部 I/O 境界。
///
/// 実機 adapter と test stub は同じ device 操作順序をこの trait で共有する。
/// 非対話条件・device 取得・secret 入力・出力はすべてこの trait を通す。
pub(crate) trait SecretsBoundary {
    type Device: SecretDevice;

    fn open_device(&mut self, serial: Option<u32>) -> Result<Self::Device>;
    fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
    ) -> Result<Self::Device>;

    /// stdin が対話入力を読める TTY かを返す。
    fn stdin_is_terminal(&self) -> bool;

    /// stdout が画面表示される TTY かを返す。
    fn stdout_is_terminal(&self) -> bool;

    /// echo なしの prompt で YubiKey PIN を読み、保護 session に所属させる。
    fn read_yubikey_pin<'session>(
        &self,
        session: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>>;

    /// echo なしの prompt で 1 行を読み、保護済み値として返す。
    fn read_hidden_secret<'session>(
        &self,
        prompt: &str,
        limit: usize,
        session: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>>;

    /// 表示 prompt で 1 行を読み、保護済み値として返す。
    fn read_visible_secret_line<'session>(
        &self,
        prompt: &str,
        limit: usize,
        session: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>>;

    /// stdin から 1 secret を読み、保護済み値として返す。
    fn read_protected_stdin_secret<'session>(
        &self,
        limit: usize,
        session: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>>;

    /// stdin JSON から 3 field を読み、保護済み enrollment secret set として返す。
    fn read_protected_enrollment_secret_set<'session>(
        &self,
        input_limit: usize,
        field_limit: usize,
        session: &'session SecretSession,
    ) -> Result<super::EnrollmentSecretSet<'session>>;

    /// 復号済み secret bytes を stdout へ書き込む。stdout が TTY の場合は停止する。
    fn write_secret_to_stdout(&self, bytes: &[u8]) -> Result<()>;

    /// summary を JSON として stdout へ出力する。
    fn write_report(&self, value: &impl serde::Serialize) -> Result<()>;

    /// TTY で yes/no prompt を表示し、応答を返す。stdin が TTY でない場合は false を返す。
    fn prompt_yes_no(&self, prompt: &str, session: &SecretSession) -> Result<bool>;
}

/// storage 操作が必要とする device API。
///
/// 実機 YubiKey と fake test double はこの最小操作を共有する。
pub(crate) trait SecretDevice {
    /// device 固有の serial。AEAD additional data にも含める。
    fn serial(&self) -> u32;
    /// secret storage 用 PIV key が存在するか確認する。
    fn key_exists(&mut self) -> Result<bool>;
    /// secret storage 用 PIV key 生成に必要な device 固有条件を確認する。
    fn check_key_generation_preconditions(&mut self) -> Result<()>;
    /// setup で永続書き込みを始める前に management key 認証可否を確認する。
    fn check_management_auth_preconditions(&mut self) -> Result<()>;
    /// secret storage 用 PIV key を device 内で生成する。
    fn generate_key(&mut self) -> Result<()>;
    /// PIV data object を読み出す。
    ///
    /// object が存在しない場合は `None` を返す。
    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>>;
    /// PIV data object に caller 所有の mutable bytes を保存する。
    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()>;
    /// content encryption key を device の public key で wrap する。
    fn wrap_key(&mut self, key: &[u8]) -> Result<Vec<u8>>;
    /// private key operation 前に application 側の PIN 入力境界を通す必要がある状態を表す。
    fn requires_pin_input(&self) -> bool;
    /// private key operation の前に、入力済み PIN で PIV session を検証する。
    fn verify_pin(&mut self, pin: &[u8]) -> Result<()>;
    /// wrapped content encryption key を device 境界内で unwrap して返す。
    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Vec<u8>>;
}
