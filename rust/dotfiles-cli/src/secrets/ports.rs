//! `dotfiles secrets` application 層が外部境界へ要求する port。
//!
//! application はこの module の trait だけに依存し、実機 YubiKey と test stub の具体的な
//! 入出力差分は adapter 側に閉じる。

use crate::Result;

use super::domain::PivObjectId;

/// application/storage 操作が必要とする device API。
///
/// 実機 YubiKey と test stub はこの最小 capability を共有し、I/O 型は port 境界へ持ち込まない。
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
///
/// 実機 adapter と test stub は同じ入力順序と device 操作順序をこの trait で共有する。
/// 非対話条件や利用者向け error contract は application 層が所有する。
pub(crate) trait SecretsBoundary {
    type Device: SecretDevice;

    fn open_device(&mut self, serial: Option<u32>) -> Result<Self::Device>;
}
