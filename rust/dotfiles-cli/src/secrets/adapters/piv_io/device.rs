use yubikey::{Serial, YubiKey};

use crate::{
    Result,
    secrets::{adapters::yubikey::YubikeySecretDevice, ports::DeviceSelectionPort},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredDevice {
    pub serial: u32,
    pub label: String,
}

pub(super) struct RealDeviceAdapter;

impl DeviceSelectionPort for RealDeviceAdapter {
    type Device = YubikeySecretDevice;
    type DeviceCandidate = DiscoveredDevice;

    fn discover_devices(&mut self) -> Result<Vec<Self::DeviceCandidate>> {
        let mut context = yubikey::Context::open()?;
        let mut devices = Vec::new();
        for reader in context.iter()? {
            let label = reader.name().into_owned();
            let yubikey = reader.open()?;
            devices.push(DiscoveredDevice {
                serial: yubikey.serial().0,
                label,
            });
        }
        Ok(devices)
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<Self::Device> {
        Ok(YubikeySecretDevice {
            yubikey: YubiKey::open_by_serial(Serial(serial))?,
            pin_verified: false,
        })
    }
}
