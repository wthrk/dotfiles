use crate::Result;
use crate::secrets::{
    domain::values::SetupCommand,
    ports::{self},
};

/// 対象 serial の YubiKey storage layout を初期化する。
///
/// setup 可否判定や PIV 操作詳細は adapter/device 側へ委譲し、application では順序制御だけを保持する。
pub(crate) fn run_setup_with<B: ports::DeviceSerialPort + ports::StorageSetupPort>(
    command: SetupCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = boundary.resolve_device_serial(command.serial)?;
    boundary.setup_storage(serial)
}
