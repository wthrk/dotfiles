use crate::Result;
use crate::secrets::{
    domain::{
        manifest::SecretManifest,
        piv::{PivObjectId, StorageObjectIds},
        values::SetupCommand,
    },
    ports::{self, SecretDevice},
};

/// 対象 serial の YubiKey storage layout を初期化する。
///
/// setup 可否判定や PIV 操作詳細は adapter/device 側へ委譲し、application では順序制御だけを保持する。
pub(crate) fn run_setup_with<B: ports::DeviceSerialPort + ports::DeviceSelectionPort>(
    command: SetupCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = boundary.resolve_device_serial(command.serial)?;
    let mut device = boundary.open_device_by_serial(serial)?;
    device.check_key_generation_preconditions()?;
    device.check_management_auth_preconditions()?;
    let key_exists = device.key_exists()?;
    let manifest_bytes = device.read_object(PivObjectId::MANIFEST)?;
    let mut occupied_object_ids = Vec::new();
    for object_id in StorageObjectIds::iter() {
        if device.read_object(object_id)?.is_some() {
            occupied_object_ids.push(object_id);
        }
    }
    SecretManifest::ensure_setup_allowed(
        key_exists,
        manifest_bytes.as_deref(),
        &occupied_object_ids,
    )?;
    device.generate_key()?;
    let mut manifest = SecretManifest::expected().encode()?;
    device.write_object(PivObjectId::MANIFEST, &mut manifest)
}
