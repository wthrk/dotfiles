//! `dotfiles secrets` application 層が外部境界へ要求する port。
//!
//! application はこの module の trait だけに依存し、実機 YubiKey と test stub の具体的な
//! 入出力差分は adapter 側に閉じる。

use crate::Result;

use super::domain::{PivObjectId, SecretName};

/// `SecretDevice` が復号して返す content encryption key。
pub(crate) struct UnwrappedContentKey {
    bytes: Vec<u8>,
}

impl UnwrappedContentKey {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.bytes
    }
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
    /// wrapped content encryption key を device 境界内で unwrap する。
    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<UnwrappedContentKey>;
}

/// application が秘密値の一時借用に使う境界型。
pub(crate) trait ProtectedSecretLike {
    fn with_secret<T>(&self, callback: impl FnOnce(&[u8]) -> T) -> T;
}

/// 登録に必要な 3 field を同じ保護 session で所有する。
pub(crate) struct EnrollmentSecretSet<T> {
    pub(crate) bw_email: T,
    pub(crate) bw_password: T,
    pub(crate) bws_access_token: T,
}

impl<T> EnrollmentSecretSet<T> {
    /// 同じ `SecretSession` に所属する 3 field から 登録対象 secretを構築する。
    pub(crate) fn new(
        bw_email: T,
        bw_password: T,
        bws_access_token: T,
    ) -> Self {
        Self {
            bw_email,
            bw_password,
            bws_access_token,
        }
    }

    #[cfg(test)]
    pub(crate) fn assert_secret_eq(&self, name: SecretName, expected: &[u8])
    where
        T: ProtectedSecretLike,
    {
        match name {
            SecretName::BwEmail => self
                .bw_email
                .with_secret(|secret| assert_eq!(secret, expected)),
            SecretName::BwPassword => self
                .bw_password
                .with_secret(|secret| assert_eq!(secret, expected)),
            SecretName::BwsAccessToken => self
                .bws_access_token
                .with_secret(|secret| assert_eq!(secret, expected)),
        }
    }
}

/// application use case が利用する外部 I/O 境界。
///
/// 実機 adapter と test stub は同じ入力順序と device 操作順序をこの trait で共有する。
/// 非対話条件や利用者向け error contract は application 層が所有する。
pub(crate) trait SecretsBoundary {
    type Device: SecretDevice;
    type InterruptSignal;
    type SecretSession;
    type ProtectedSecret<'session>: ProtectedSecretLike;

    fn stdin_is_terminal(&self) -> bool;
    fn stdout_is_terminal(&self) -> bool;
    fn open_device(&mut self, serial: Option<u32>) -> Result<Self::Device>;
    fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
        interrupt: &Self::InterruptSignal,
    ) -> Result<Self::Device>;
    fn read_enrollment_secret_set<'session>(
        &mut self,
        stdin_json: bool,
        memory: &'session Self::SecretSession,
    ) -> Result<EnrollmentSecretSet<Self::ProtectedSecret<'session>>>;
    fn read_secret_for_put<'session>(
        &mut self,
        name: SecretName,
        stdin: bool,
        memory: &'session Self::SecretSession,
    ) -> Result<Self::ProtectedSecret<'session>>;
    fn read_yubikey_pin<'session>(
        &mut self,
        memory: &'session Self::SecretSession,
    ) -> Result<Self::ProtectedSecret<'session>>;
    fn prompt_yes_no(&mut self, prompt: &str, interrupt: &Self::InterruptSignal) -> Result<bool>;
}
