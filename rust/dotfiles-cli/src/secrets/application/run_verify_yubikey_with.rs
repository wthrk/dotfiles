use crate::Result;
use crate::secrets::{
    domain::{
        piv::SecretStorageSpec,
        storage::SecretStorageReadIntent,
        values::{VerifySummary, VerifyYubikeyCommand},
    },
    ports::{self, SecretStoragePort},
};

/// 保存済み secret の存在と、要求された外部確認項目を検証する。
///
/// local storage 検証を完了条件の先頭に固定し、未実装の外部確認は report 境界で通知して
/// 明示的に停止することで、verify 結果の責任範囲を曖昧にしない。
pub(crate) fn run_verify_yubikey_with<
    B: ports::DevicePinPolicyPort + ports::PinInputPort + SecretStoragePort + ports::ReportPort,
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
    for storage in SecretStorageSpec::all_for_serial(serial) {
        let inspection = boundary.inspect_secret_storage_read(serial, &storage)?;
        let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
        let _secret = boundary.load_secret(serial, intent, pin.as_ref())?;
    }
    if !requested.is_empty() {
        boundary.write_verify_report(&VerifySummary::external_checks_unavailable(
            serial,
            requested.iter().copied(),
        ))?;
        return Err(command.external_checks_unavailable_error(&requested));
    }

    boundary.write_verify_report(&VerifySummary::local_storage_verified(serial))
}
