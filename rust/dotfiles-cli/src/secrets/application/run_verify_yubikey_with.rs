use anyhow::bail;

use crate::Result;
use crate::secrets::{
    domain::{
        manifest::SecretManifest,
        piv::{PivObjectId, SecretStorageSpec},
        values::{VerifySummary, VerifyYubikeyCommand},
    },
    ports::{self, SecretDevice},
};

/// 保存済み secret の存在と、要求された外部確認項目を検証する。
///
/// local storage 検証を完了条件の先頭に固定し、未実装の外部確認は report 境界で通知して
/// 明示的に停止することで、verify 結果の責任範囲を曖昧にしない。
pub(crate) fn run_verify_yubikey_with<
    B: ports::DevicePinPolicyPort
        + ports::PinInputPort
        + ports::DeviceSelectionPort
        + ports::ReportPort,
>(
    command: VerifyYubikeyCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = command.required_serial()?;
    let requested = command.requested_external_checks()?;
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
    for storage in SecretStorageSpec::all_for_serial(serial) {
        let encoded = device
            .read_object(storage.object_id)?
            .ok_or_else(|| storage.missing_error())?;
        let _secret = device
            .open_from_storage(storage.clone(), &encoded)
            .map_err(|error| storage.decode_error(error))?;
    }
    if !requested.is_empty() {
        boundary.write_verify_report(&VerifySummary::external_checks_unavailable(
            serial,
            requested.iter().copied(),
        ))?;
        let requested_names = requested
            .iter()
            .map(|check| check.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        bail!("external checks are not implemented yet: {requested_names}");
    }

    boundary.write_verify_report(&VerifySummary::local_storage_verified(serial))
}
