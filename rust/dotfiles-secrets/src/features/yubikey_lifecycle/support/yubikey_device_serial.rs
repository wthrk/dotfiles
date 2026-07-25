//! YubiKey device discovery の technical serial 解決。

use crate::{
    Result,
    features::yubikey_lifecycle::domain::piv::PivDeviceProfile,
    features::yubikey_lifecycle::support::{
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
    let devices = yubikey_backend::discover_devices(backend)?;
    if let Some(serial) = requested {
        if let Some(candidate) = devices.into_iter().find(|device| device.serial == serial) {
            yubikey_backend::bind_discovery_step(backend, candidate);
            return Ok(serial);
        }
        anyhow::bail!("requested YubiKey serial is not connected");
    }
    let serial = resolve_exactly_one_serial(
        &devices
            .iter()
            .map(|device| device.serial)
            .collect::<Vec<_>>(),
        MULTIPLE_DEVICES_ERROR,
    )?;
    let candidate = devices
        .into_iter()
        .find(|device| device.serial == serial)
        .ok_or_else(|| anyhow::anyhow!("resolved YubiKey disappeared from discovery snapshot"))?;
    yubikey_backend::bind_discovery_step(backend, candidate);
    Ok(serial)
}

pub(crate) fn inspect_device_profile(
    backend: &mut YubikeyDeviceBackend,
    serial: u32,
) -> Result<PivDeviceProfile> {
    Ok(yubikey_backend::take_discovery_step(backend, serial)?.profile)
}
