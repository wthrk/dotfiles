use anyhow::bail;

use crate::Result;
use crate::secrets::{
    domain::{
        manifest::SecretManifest,
        piv::PivObjectId,
        piv::SecretName,
        values::{RotateBwsTokenCommand, VerifySummary},
    },
    ports::{self, SecretDevice},
};

const NONINTERACTIVE_SERIAL_ERROR: &str = "pass --serial in non-interactive use";

/// stdin 入力で BWS token を更新し、YubiKey 保存状態を再検証する。
///
/// token 読み取り方式は port 境界で差し替え、use case 側では serial 必須条件と
/// 保存後検証の順序のみを固定して責務混在を避ける。
pub(crate) fn run_rotate_bws_token_with_stdin<
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
    let token = boundary.read_stdin_secret()?;
    let mut device = boundary.open_device_by_serial(serial)?;
    SecretManifest::decode_initialized(device.read_object(PivObjectId::MANIFEST)?.as_deref())?;
    device.check_management_auth_preconditions()?;
    let mut encoded = device.seal_for_storage(SecretName::BwsAccessToken, &token)?;
    device.write_object(SecretName::BwsAccessToken.object_id(), &mut encoded)?;
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
        for name in SecretName::iter() {
            let encoded = verify_device
                .read_object(name.object_id())?
                .ok_or_else(|| anyhow::anyhow!("{name} is not stored on this YubiKey"))?;
            let _secret = verify_device
                .open_from_storage(name, &encoded)
                .map_err(|error| anyhow::anyhow!("failed to decode {name}: {error}"))?;
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
