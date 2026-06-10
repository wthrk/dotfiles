//! YubiKey device 選択を `DeviceSerialPort` へ翻訳する adapter。
//!
//! 接続済み device の内部識別子解決と複数接続拒否だけを担当し、secret 保存・復号・report 生成は他 adapter へ分離する。

use anyhow::bail;

use crate::{
    Result,
    secrets::ports::yubikey::{DevicePinPolicyPort, DeviceSerialPort},
};

use super::{
    DeviceCandidate, SecretDeviceIo, SelectedDeviceAdapter, SelectedDeviceDiscoveryIo,
    SelectedSecretDevice,
};

const MULTIPLE_DEVICES_ERROR: &str =
    "multiple YubiKeys detected; connect exactly one YubiKey and retry";

/// YubiKey discovery と device 識別子解決 port を実 device enumeration へ翻訳する adapter。
///
/// adapter は複数接続時の停止をこの境界に閉じ、storage intent や secret 読み書きの業務判断を持たない。
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
}

impl DeviceSerialPort for DeviceSelectionAdapter {
    fn resolve_device_serial(&mut self) -> Result<u32> {
        let devices = self.discover_devices()?;
        resolve_discovered_device_serial(&devices)
    }
}

impl DevicePinPolicyPort for DeviceSelectionAdapter {
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool> {
        let device = self.open_device_by_serial(serial)?;
        Ok(device.requires_pin_input())
    }
}

fn resolve_discovered_device_serial(devices: &[DeviceCandidate]) -> Result<u32> {
    match devices {
        [] => bail!("no YubiKey detected"),
        [device] => Ok(device.serial),
        _ => bail!(MULTIPLE_DEVICES_ERROR),
    }
}

#[cfg(test)]
mod tests {
    //! device selection adapter が複数接続時に識別子表示や番号による対象指定へ進まないことを検証する。

    use super::{MULTIPLE_DEVICES_ERROR, resolve_discovered_device_serial};
    use crate::secrets::adapters::yubikey::DeviceCandidate;

    fn ok<T, E: std::fmt::Display>(result: std::result::Result<T, E>, context: &str) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("expected {context} to succeed: {error}"),
        }
    }

    /// 単一接続時は利用者選択を挟まず、その device serial を port 境界へ返す。
    #[test]
    fn single_discovered_device_is_selected_without_prompt() {
        let devices = [DeviceCandidate {
            serial: 2001,
            label: "YubiKey 5".to_string(),
        }];

        let selected = ok(resolve_discovered_device_serial(&devices), "single device");

        assert_eq!(selected, 2001);
    }

    /// 複数接続時は serial や index の選択肢を出さず、接続数を 1 件にする操作制約で停止する。
    #[test]
    fn multiple_devices_stop_without_identifier_or_index_selection() {
        let devices = [
            DeviceCandidate {
                serial: 2001,
                label: "first reader".to_string(),
            },
            DeviceCandidate {
                serial: 2002,
                label: "second reader".to_string(),
            },
        ];

        let error = resolve_discovered_device_serial(&devices)
            .expect_err("multiple devices must stop")
            .to_string();

        assert_eq!(error, MULTIPLE_DEVICES_ERROR);
        assert!(
            error.contains("connect exactly one YubiKey"),
            "multiple-device error must require a single connected target"
        );
        let identifier_word = ['s', 'e', 'r', 'i', 'a', 'l'].iter().collect::<String>();
        let identifier_option = format!("--{identifier_word}");
        let forbidden = [
            identifier_word.as_str(),
            identifier_option.as_str(),
            "1:",
            "2:",
            "number",
            "choose",
            "select",
        ];
        for forbidden in forbidden {
            assert!(
                !error.contains(forbidden),
                "multiple-device error must not expose identifiers or offer index selection"
            );
        }
    }
}
