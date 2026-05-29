//! YubiKey device 選択を `DeviceSerialPort`/`SpareDeviceSerialPort` へ翻訳する adapter。
//!
//! serial 解決と対話選択のみを担当し、secret 保存・復号・report 生成は他 adapter へ分離する。

use anyhow::{Context, bail};

use crate::{
    Result,
    secrets::{
        ports::yubikey::{DevicePinPolicyPort, DeviceSerialPort, SpareDeviceSerialPort},
        support::process_io,
    },
};

use super::{
    DeviceCandidate, SecretDeviceIo, SelectedDeviceAdapter, SelectedDeviceDiscoveryIo,
    SelectedSecretDevice,
};

/// YubiKey discovery と serial 解決 port を実 device enumeration へ翻訳する adapter。
///
/// caller は serial 指定有無だけを渡す。adapter は対話選択と非対話拒否をこの境界に閉じ、
/// storage intent や secret 読み書きの業務判断を持たない。
#[derive(Default)]
pub(super) struct DeviceSelectionAdapter {
    device: SelectedDeviceAdapter,
}

impl DeviceSelectionAdapter {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>> {
        SelectedDeviceDiscoveryIo::discover_devices(&mut self.device)
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<SelectedSecretDevice> {
        SelectedDeviceDiscoveryIo::open_device_by_serial(&mut self.device, serial)
    }

    fn select_device_interactively(&mut self, devices: &[DeviceCandidate]) -> Result<u32> {
        eprintln!("multiple YubiKeys detected:");
        for (index, device) in devices.iter().enumerate() {
            let number = index + 1;
            eprintln!("{number}: serial {} ({})", device.serial, device.label);
        }
        let selection = process_io::read_control_line("select YubiKey number: ")?;
        let selection = selection
            .trim()
            .parse::<usize>()
            .context("selected YubiKey number is invalid")?;
        let Some(device) = devices.get(selection.saturating_sub(1)) else {
            bail!("selected YubiKey number is out of range");
        };
        Ok(device.serial)
    }
}

impl DeviceSerialPort for DeviceSelectionAdapter {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
        if let Some(serial) = requested {
            return Ok(serial);
        }
        let devices = self.discover_devices()?;
        match devices.as_slice() {
            [] => bail!("no YubiKey detected"),
            [device] => Ok(device.serial),
            _ if process_io::stdin_is_terminal() => self.select_device_interactively(&devices),
            _ => bail!("multiple YubiKeys detected; pass --serial to select a device"),
        }
    }
}

impl SpareDeviceSerialPort for DeviceSelectionAdapter {
    fn resolve_spare_device_serial(&mut self, requested_spare_serial: Option<u32>) -> Result<u32> {
        self.resolve_device_serial(requested_spare_serial)
    }
}

impl DevicePinPolicyPort for DeviceSelectionAdapter {
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool> {
        let device = self.open_device_by_serial(serial)?;
        Ok(device.requires_pin_input())
    }
}
