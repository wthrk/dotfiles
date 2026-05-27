use anyhow::Result;
use yubikey::YubiKey;

use super::ProtectedSecret;

pub(crate) fn verify(yubikey: &mut YubiKey, pin: &ProtectedSecret) -> Result<()> {
    pin.with_secret(|pin_bytes| yubikey.verify_pin(pin_bytes).map_err(anyhow::Error::new))
}
