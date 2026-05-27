use anyhow::bail;

use crate::Result;
use crate::secrets::{
    domain::{manifest::SecretManifest, piv::PivObjectId, values::GetCommand},
    ports::{self, SecretDevice},
};

/// 指定された secret を YubiKey storage から読み出し、出力 port へ受け渡す。
///
/// 読み出し経路の secret 値を application 層で加工せず、復号と出力方針は adapter 側の責務境界へ固定する。
pub(crate) fn run_get_with<
    B: ports::DeviceSerialPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + ports::DeviceSelectionPort
        + ports::SecretOutputPort,
>(
    command: GetCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = boundary.resolve_device_serial(command.serial)?;
    let pin = if boundary.device_requires_pin(serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    let mut device = boundary.open_device_by_serial(serial)?;
    if device.requires_pin_input() {
        let Some(pin) = pin.as_ref() else {
            bail!("PIN is required for this operation");
        };
        device.verify_pin(pin)?;
    }
    SecretManifest::decode_initialized(device.read_object(PivObjectId::MANIFEST)?.as_deref())?;
    let storage = command.storage_spec(serial);
    let encoded = device
        .read_object(storage.object_id)?
        .ok_or_else(|| storage.missing_error())?;
    let secret = device
        .open_from_storage(storage.clone(), &encoded)
        .map_err(|error| storage.decode_error(error))?;
    boundary.write_secret(&secret)
}
