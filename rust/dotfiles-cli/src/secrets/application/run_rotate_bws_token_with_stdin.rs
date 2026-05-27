use crate::Result;
use crate::secrets::{
    domain::values::{RotateBwsTokenCommand, VerifySummary},
    ports::{self, SecretStoragePort},
};

/// stdin 入力で BWS token を更新し、YubiKey 保存状態を再検証する。
///
/// token 読み取り方式は port 境界で差し替え、use case 側では serial 必須条件と
/// 保存後検証の順序のみを固定して責務混在を避ける。
pub(crate) fn run_rotate_bws_token_with_stdin<
    B: ports::SecretInputPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + SecretStoragePort
        + ports::ReportPort,
>(
    command: RotateBwsTokenCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = command.required_serial()?;
    let token = boundary.read_stdin_secret()?;
    let storage = command.storage_spec(serial);
    boundary.store_secret(serial, storage, &token)?;
    let pin = if boundary.device_requires_pin(serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    let verify_result = boundary.verify_local_storage(serial, pin.as_ref());
    match verify_result {
        Ok(()) => boundary.write_verify_report(&VerifySummary::local_storage_verified(serial)),
        Err(err) => boundary
            .write_verify_report(&VerifySummary::local_storage_failed(serial))
            .and(Err(err)),
    }
}
