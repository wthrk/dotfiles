//! YubiKey management commands が必要とする PIN input capability。
//!
//! PIV PIN は YubiKey lifecycle の管理 flow にだけ属する。TTY 表示は presentation
//! receiver が所有し、この contract は application が要求する protected value だけを表す。

use crate::{Result, foundation::protection::ProtectedSecret};

#[cfg_attr(test, mockall::automock)]
pub(crate) trait PivPinInputPort {
    fn read_piv_pin_secret(&self) -> Result<ProtectedSecret>;
    fn read_current_piv_pin_secret(&self) -> Result<ProtectedSecret>;
    fn read_new_piv_pin_confirmation(&self) -> Result<ProtectedSecret>;
}
