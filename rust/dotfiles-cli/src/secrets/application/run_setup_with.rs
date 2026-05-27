use crate::Result;
use crate::secrets::{
    domain::{
        storage::{SecretStorageSetupIntent, SecretStorageSetupProbe},
        values::SetupCommand,
    },
    ports::{self, SecretStoragePort},
};

/// 対象 serial の YubiKey storage layout を初期化する。
///
/// setup 可否判定や PIV 操作詳細は adapter/device 側へ委譲し、application では順序制御だけを保持する。
pub(crate) fn run_setup_with<B: ports::DeviceSerialPort + SecretStoragePort>(
    command: SetupCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = boundary.resolve_device_serial(command.serial)?;
    let probe = SecretStorageSetupProbe::expected();
    let inspection = boundary.inspect_secret_storage_setup(serial, &probe)?;
    let intent = SecretStorageSetupIntent::from_inspection(inspection)?;
    boundary.initialize_secret_storage(serial, intent)
}
