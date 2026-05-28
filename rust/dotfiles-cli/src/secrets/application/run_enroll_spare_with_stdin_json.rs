//! enroll-spare(stdin-json) の順序を固定し、device 衝突停止条件を入力方式に依存させない。

use crate::Result;
use crate::secrets::{
    domain::{
        manifest::BootstrapSecretDocument,
        piv::validate_piv_pin_len,
        storage::{
            SecretStorageReadIntent, SecretStorageSetupIntent, SecretStorageSetupProbe,
            SecretStorageVerificationPlan, SecretStorageWriteIntent,
        },
        values::{EnrollSpareCommand, EnrollSummary},
    },
    ports::{self, SecretStoragePort},
};

/// stdin JSON document で spare YubiKey に bootstrap secret 一式を登録する。
///
/// primary と spare の衝突停止条件を先に評価し、device 選択・入力実装は port に委譲して
/// use case の順序責務だけを保持する。
pub(crate) fn run_enroll_spare_with_stdin_json<
    B: ports::SpareDeviceSerialPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + ports::BootstrapSecretDocumentInputPort
        + SecretStoragePort
        + ports::ReportPort,
>(
    command: EnrollSpareCommand,
    boundary: &mut B,
) -> Result<()> {
    command.ensure_requested_serials_distinct()?;
    let spare_serial = boundary.resolve_spare_device_serial(command.spare_serial)?;
    command.ensure_requested_primary_differs_from_spare(spare_serial)?;
    let setup_probe = SecretStorageSetupProbe::expected();
    let setup_inspection = boundary.inspect_secret_storage_setup(spare_serial, &setup_probe)?;
    let setup_intent = SecretStorageSetupIntent::from_inspection(setup_inspection)?;
    let fields = boundary.read_bootstrap_secret_fields()?;
    let document = BootstrapSecretDocument::from_field_map(fields)?;
    boundary.initialize_secret_storage(spare_serial, setup_intent.clone())?;
    for (storage, value) in document.storage_entries(spare_serial) {
        let intent = SecretStorageWriteIntent::initial_enroll_store(storage, value.len())?;
        boundary.store_secret(spare_serial, intent, value)?;
    }
    boundary.finalize_secret_storage_setup(spare_serial, setup_intent)?;
    let pin = if boundary.device_requires_pin(spare_serial)? {
        let pin = boundary.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };
    for storage in SecretStorageVerificationPlan::for_serial(spare_serial).into_targets() {
        let inspection = boundary.inspect_secret_storage_read(spare_serial, &storage)?;
        let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
        let secret = boundary
            .load_secret(spare_serial, &intent, pin.as_ref())
            .map_err(|error| intent.decode_error(error))?;
        intent.validate_loaded_secret(&secret)?;
    }
    boundary.write_enroll_report(&EnrollSummary::spare_completed(spare_serial))
}

#[cfg(test)]
mod tests {
    use crate::Result;
    use crate::secrets::{
        application::app_test_support::AppMockBoundary, domain::values::EnrollSpareCommand,
    };

    use super::run_enroll_spare_with_stdin_json;

    #[test]
    fn enroll_spare_stdin_json_rejects_same_requested_serial_before_device_resolution() {
        let mut boundary = AppMockBoundary::new();
        let result = run_enroll_spare_with_stdin_json(
            EnrollSpareCommand {
                primary_serial: Some(2002),
                spare_serial: Some(2002),
            },
            &mut boundary,
        );
        assert!(result.is_err(), "same primary/spare serial should stop");
    }

    #[test]
    fn enroll_spare_stdin_json_reads_pin_for_verify_when_required() -> Result<()> {
        let mut boundary = AppMockBoundary::new()
            .expect_enrollment_success()
            .expect_pin();
        boundary.mock.set_spare_requires_pin(true);
        run_enroll_spare_with_stdin_json(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2002),
            },
            &mut boundary,
        )
    }

    #[test]
    fn enroll_spare_stdin_json_skips_pin_when_not_required() -> Result<()> {
        let mut boundary = AppMockBoundary::new().expect_enrollment_success();
        run_enroll_spare_with_stdin_json(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2002),
            },
            &mut boundary,
        )
    }

    #[test]
    fn enroll_spare_stdin_json_stops_when_verify_fails() {
        let mut boundary = AppMockBoundary::new();
        boundary.mock.expect_event("setup");
        boundary.mock.expect_event_times("store", 3);
        boundary.mock.set_loaded_len(0);
        let result = run_enroll_spare_with_stdin_json(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2002),
            },
            &mut boundary,
        );
        assert!(result.is_err(), "verify error should stop use case");
    }
}
