use crate::Result;
use crate::secrets::{
    domain::{
        manifest::BootstrapSecretDocument,
        piv::SecretName,
        values::{EnrollPrimaryCommand, EnrollSummary},
    },
    ports::{self, SecretStoragePort},
};

/// prompt 入力で primary YubiKey に bootstrap secret 一式を登録する。
///
/// 入力手段の詳細は `SecretInputPort` 側へ閉じ込め、use case は setup→store→verify の
/// 順序制御だけを担って application 層の責務境界を維持する。
pub(crate) fn run_enroll_primary_with_prompt<
    B: ports::DeviceSerialPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + ports::SecretInputPort
        + SecretStoragePort
        + ports::ReportPort,
>(
    command: EnrollPrimaryCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = boundary.resolve_device_serial(command.serial)?;
    boundary.initialize_secret_storage(serial)?;
    let bw_email = boundary.read_visible_secret()?;
    let bw_password = boundary.read_hidden_secret(SecretName::BwPassword)?;
    let bws_access_token = boundary.read_hidden_secret(SecretName::BwsAccessToken)?;
    let document =
        BootstrapSecretDocument::from_secret_materials(&bw_email, &bw_password, &bws_access_token)?;
    for (storage, value) in document.storage_entries(serial) {
        boundary.store_secret(serial, storage, value)?;
    }
    let pin = if boundary.device_requires_pin(serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    boundary.verify_local_storage(serial, pin.as_ref())?;
    boundary.write_enroll_report(&EnrollSummary::primary_completed(serial))
}
