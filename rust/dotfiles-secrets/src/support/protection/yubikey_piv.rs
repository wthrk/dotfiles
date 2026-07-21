//! PIV PIN / management-key operation を protection 境界内で完結する。

use crate::{Result, support::protection::ProtectedSecret};
use anyhow::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProtectedManagementKeyState {
    Authenticated,
    Missing,
}
pub(crate) fn verify_pin(yubikey: &mut yubikey::YubiKey, pin: &ProtectedSecret) -> Result<()> {
    pin.with_secret(|bytes| yubikey.verify_pin(bytes).map_err(Error::new))
}
pub(crate) fn authenticate_protected_management_key(
    yubikey: &mut yubikey::YubiKey,
    pin: &ProtectedSecret,
) -> Result<ProtectedManagementKeyState> {
    verify_pin(yubikey, pin)?;
    match yubikey::MgmKey::get_protected(yubikey) {
        Ok(key) => {
            yubikey.authenticate(&key).map_err(Error::new)?;
            Ok(ProtectedManagementKeyState::Authenticated)
        }
        Err(yubikey::Error::NotFound) => Ok(ProtectedManagementKeyState::Missing),
        Err(error) => Err(Error::new(error)),
    }
}
pub(crate) fn bootstrap_pin_protected_management_key(yubikey: &mut yubikey::YubiKey) -> Result<()> {
    let default = yubikey::MgmKey::get_default(yubikey).map_err(Error::new)?;
    yubikey.authenticate(&default).map_err(Error::new)?;
    let protected = yubikey::MgmKey::generate_for(yubikey, &mut yubikey_rand::rngs::SysRng)
        .map_err(Error::new)?;
    protected.set_protected(yubikey).map_err(Error::new)?;
    Ok(())
}
