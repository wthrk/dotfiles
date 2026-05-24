//! `dotfiles secrets` application 層が外部境界へ要求する port。
//!
//! application はこの module の trait に依存し、実装詳細は adapter 側に閉じる。

use crate::Result;

use super::domain::{PivObjectId, SecretName};

/// storage 操作が必要とする device API。
pub(crate) trait SecretDevice {
    fn serial(&self) -> u32;
    fn key_exists(&mut self) -> Result<bool>;
    fn check_key_generation_preconditions(&mut self) -> Result<()>;
    fn check_management_auth_preconditions(&mut self) -> Result<()>;
    fn generate_key(&mut self) -> Result<()>;
    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>>;
    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()>;
    fn wrap_key(&mut self, key: &[u8]) -> Result<Vec<u8>>;
    fn requires_pin_input(&self) -> bool;
    fn verify_pin(&mut self, pin: &[u8]) -> Result<()>;
    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Vec<u8>>;
}

/// application use case が利用する外部 I/O 境界。
pub(crate) trait SecretsBoundary {
    type Device: SecretDevice;

    fn stdin_is_terminal(&self) -> bool;
    fn stdout_is_terminal(&self) -> bool;
    fn open_device(&mut self, serial: Option<u32>) -> Result<Self::Device>;
    fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
    ) -> Result<Self::Device>;
    fn read_enrollment_secret_set(
        &mut self,
        stdin_json: bool,
    ) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>)>;
    fn read_secret_for_put(&mut self, name: SecretName, stdin: bool) -> Result<Vec<u8>>;
    fn read_yubikey_pin(&mut self) -> Result<Vec<u8>>;
    fn confirm_update_another_yubikey(&mut self) -> Result<bool>;
    fn write_secret_output(&mut self, bytes: &[u8]) -> Result<()>;
    fn write_json_report(&mut self, value: &impl serde::Serialize) -> Result<()>;
}
