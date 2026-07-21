//! YubiKey device discovery の technical serial 解決。

use crate::{
    Result,
    support::{
        piv_storage::resolve_exactly_one_serial,
        yubikey_backend::{self, YubikeyDeviceBackend},
    },
};

const MULTIPLE_DEVICES_ERROR: &str =
    "multiple YubiKeys detected; connect exactly one YubiKey and retry";

pub(crate) fn resolve_device_serial(
    backend: &mut YubikeyDeviceBackend,
    requested: Option<u32>,
) -> Result<u32> {
    if let Some(serial) = requested {
        return Ok(serial);
    }
    let devices = yubikey_backend::discover_devices(backend)?;
    resolve_exactly_one_serial(
        &devices
            .iter()
            .map(|device| device.serial)
            .collect::<Vec<_>>(),
        MULTIPLE_DEVICES_ERROR,
    )
}
