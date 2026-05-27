use crate::Result;
use crate::secrets::{
    domain::values::GetCommand,
    ports::{self, SecretStoragePort},
};

/// 指定された secret を YubiKey storage から読み出し、出力 port へ受け渡す。
///
/// 読み出し経路の secret 値を application 層で加工せず、復号と出力方針は adapter 側の責務境界へ固定する。
pub(crate) fn run_get_with<
    B: ports::DeviceSerialPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + SecretStoragePort
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
    let storage = command.storage_spec(serial);
    let secret = boundary.load_secret(serial, storage, pin.as_ref())?;
    boundary.write_secret(&secret)
}
