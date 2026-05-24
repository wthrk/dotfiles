//! `dotfiles secrets` application 層が外部境界へ要求する port。
//!
//! application はこの module の trait だけに依存し、実機 YubiKey と test stub の具体的な
//! 入出力差分は adapter 側に閉じる。

use crate::Result;

use super::domain::PivObjectId;

/// application/storage 操作が要求する最小の YubiKey device capability 契約。
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
