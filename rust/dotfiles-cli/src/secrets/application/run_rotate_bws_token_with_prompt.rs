use crate::Result;
use crate::secrets::{
    domain::{
        piv::SecretStorageSpec,
        storage::{SecretStorageReadIntent, SecretStorageWriteIntent},
        values::{RotateBwsTokenCommand, VerifySummary},
    },
    ports::{self, SecretStoragePort},
};

/// prompt 入力で BWS token を更新し、YubiKey 保存状態を再検証する。
///
/// serial 未指定時は非対話運用の誤書き込みを防ぐため停止し、保存失敗と検証失敗の責務は
/// port 境界で保存と検証を接続する。
pub(crate) fn run_rotate_bws_token_with_prompt<
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
    let token = boundary.read_hidden_secret(command.target_secret())?;
    let storage = command.storage_spec(serial);
    let inspection = boundary.inspect_secret_storage_write(serial, &storage)?;
    let intent = SecretStorageWriteIntent::store(storage, inspection)?;
    boundary.store_secret(serial, intent, &token)?;
    let pin = if boundary.device_requires_pin(serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    let verify_result: Result<()> = (|| {
        for storage in SecretStorageSpec::all_for_serial(serial) {
            let inspection = boundary.inspect_secret_storage_read(serial, &storage)?;
            let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
            let _secret = boundary.load_secret(serial, intent, pin.as_ref())?;
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
