//! YubiKey device 選択を `DeviceSerialPort` へ翻訳する adapter。
//!
//! 明示 serial または単一接続 device の内部識別子解決だけを担当し、secret 保存・復号・report 生成は他
//! adapter へ分離する。未指定時に複数接続を検出した場合は、識別子表示や番号選択に進まず停止する。

use anyhow::bail;

use crate::{Result, ports::yubikey::DeviceSerialPort};

use super::{DeviceCandidate, SelectedDeviceAdapter, SelectedDeviceDiscoveryIo};

const MULTIPLE_DEVICES_ERROR: &str =
    "multiple YubiKeys detected; connect exactly one YubiKey and retry";

/// YubiKey discovery と serial 解決 port を実 device enumeration へ翻訳する adapter。
///
/// caller は serial 指定有無だけを渡す。adapter は未指定時の単一接続解決と複数接続拒否をこの境界に
/// 閉じ、storage intent や secret 読み書きの業務判断を持たない。
#[derive(Default)]
pub(super) struct DeviceSelectionAdapter {
    device: SelectedDeviceAdapter,
}

impl DeviceSelectionAdapter {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>> {
        SelectedDeviceDiscoveryIo::discover_devices(&mut self.device)
    }
}

impl DeviceSerialPort for DeviceSelectionAdapter {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
        if let Some(serial) = requested {
            return Ok(serial);
        }
        let devices = self.discover_devices()?;
        resolve_discovered_device_serial(&devices)
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
    //! device selection adapter が複数接続時に識別子表示や番号選択へ進まないことを検証する。

    use super::{MULTIPLE_DEVICES_ERROR, resolve_discovered_device_serial};
    use crate::adapters::yubikey::DeviceCandidate;

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

        let error = match resolve_discovered_device_serial(&devices) {
            Ok(serial) => panic!("expected multiple devices to stop, selected {serial}"),
            Err(error) => error.to_string(),
        };

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
