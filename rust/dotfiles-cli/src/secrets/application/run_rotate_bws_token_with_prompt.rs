use anyhow::bail;

use crate::Result;
use crate::secrets::{
    domain::{
        manifest::SecretManifest,
        piv::{PivObjectId, SecretStorageSpec},
        values::{RotateBwsTokenCommand, VerifySummary},
    },
    ports::{self, SecretDevice},
};

const NONINTERACTIVE_SERIAL_ERROR: &str = "pass --serial in non-interactive use";

/// prompt 入力で BWS token を更新し、YubiKey 保存状態を再検証する。
///
/// serial 未指定時は非対話運用の誤書き込みを防ぐため停止し、保存失敗と検証失敗の責務は
/// port 境界で保存と検証を接続する。
pub(crate) fn run_rotate_bws_token_with_prompt<
    B: ports::SecretInputPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + ports::DeviceSelectionPort
        + ports::ReportPort,
>(
    command: RotateBwsTokenCommand,
    boundary: &mut B,
) -> Result<()> {
    let Some(serial) = command.serial else {
        bail!(NONINTERACTIVE_SERIAL_ERROR);
    };
    let token = boundary.read_hidden_secret(command.target_secret())?;
    let mut device = boundary.open_device_by_serial(serial)?;
    SecretManifest::decode_initialized(device.read_object(PivObjectId::MANIFEST)?.as_deref())?;
    device.check_management_auth_preconditions()?;
    let storage = command.storage_spec(serial);
    let mut encoded = device.seal_for_storage(storage.clone(), &token)?;
    device.write_object(storage.object_id, &mut encoded)?;
    let pin = if boundary.device_requires_pin(serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    let mut verify_device = boundary.open_device_by_serial(serial)?;
    if verify_device.requires_pin_input() {
        let Some(pin) = pin.as_ref() else {
            bail!("PIN is required for this operation");
        };
        verify_device.verify_pin(pin)?;
    }
    let verify_result = (|| -> Result<()> {
        SecretManifest::decode_initialized(
            verify_device.read_object(PivObjectId::MANIFEST)?.as_deref(),
        )?;
        for storage in SecretStorageSpec::all_for_serial(serial) {
            let encoded = verify_device
                .read_object(storage.object_id)?
                .ok_or_else(|| storage.missing_error())?;
            let _secret = verify_device
                .open_from_storage(storage.clone(), &encoded)
                .map_err(|error| storage.decode_error(error))?;
        }
        Ok(())
    })();
    match verify_result {
        Ok(()) => boundary.write_verify_report(&VerifySummary::local_storage_verified(serial)),
        Err(err) => boundary
            .write_verify_report(&VerifySummary::local_storage_failed(serial))
            .and(Err(err)),
    }
}
