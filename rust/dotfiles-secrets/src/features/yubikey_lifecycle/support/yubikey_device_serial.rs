//! YubiKey device discovery の technical serial 解決。

use crate::{
    Result,
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

pub(crate) fn preflight_device_profile(
    backend: &mut YubikeyDeviceBackend,
    serial: u32,
) -> Result<()> {
    let profile = yubikey_backend::bound_device_profile(backend, serial)
        .or_else(|| {
            yubikey_backend::discover_devices_uncached()
                .ok()?
                .into_iter()
                .find_map(|candidate| (candidate.serial == serial).then_some(candidate.profile))
        })
        .ok_or_else(|| {
            anyhow::anyhow!("YubiKey profile could not be resolved for serial {serial}")
        })?;

    profile.ensure_pin_free_recovery_supported()
}
