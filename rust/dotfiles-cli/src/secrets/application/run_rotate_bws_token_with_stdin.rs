//! rotate-bws-token stdin use case の orchestration。

use crate::Result;
use crate::secrets::{
    domain::{
        piv::validate_piv_pin_len,
        storage::{
            SecretStorageReadIntent, SecretStorageVerificationPlan, SecretStorageWriteIntent,
        },
        values::{RotateBwsTokenCommand, VerifySummary},
    },
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
    let token = boundary.read_streamed_secret()?;
    let storage = command.storage_spec(serial);
    let inspection = boundary.inspect_secret_storage_write(serial, &storage)?;
    let intent = SecretStorageWriteIntent::store(storage, inspection, token.len())?;
    boundary.store_secret(serial, intent, &token)?;
    let pin = if boundary.device_requires_pin(serial)? {
        let pin = boundary.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };
    let verify_result: Result<()> = (|| {
        for storage in SecretStorageVerificationPlan::for_serial(serial).into_targets() {
            let inspection = boundary.inspect_secret_storage_read(serial, &storage)?;
            let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
            let secret = boundary
                .load_secret(serial, &intent, pin.as_ref())
                .map_err(|error| intent.decode_error(error))?;
            intent.validate_loaded_secret(&secret)?;
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

#[cfg(all(test, feature = "secrets-internal-test-stub"))]
mod tests {
    use crate::Result;
    use crate::secrets::{
        application::app_test_support::AppMockBoundary,
        domain::values::{CheckName, CheckStatus, RotateBwsTokenCommand},
    };

    use super::run_rotate_bws_token_with_stdin;

    #[test]
    fn rotate_stdin_stops_without_serial() {
        let mut boundary = AppMockBoundary::new();
        let result =
            run_rotate_bws_token_with_stdin(RotateBwsTokenCommand { serial: None }, &mut boundary);
        assert!(result.is_err(), "serial is required for stdin rotation");
    }

    #[test]
    fn rotate_stdin_reports_verify_success_and_failure() -> Result<()> {
        let mut success = AppMockBoundary::new().expect_rotation_success();
        run_rotate_bws_token_with_stdin(
            RotateBwsTokenCommand { serial: Some(2001) },
            &mut success,
        )?;
        let success_reports = success.mock.reports();
        assert_eq!(
            success_reports[0].checks.get(&CheckName::LocalStorage),
            Some(&CheckStatus::Ok)
        );

        let mut failed = AppMockBoundary::new().expect_rotation_success();
        failed.mock.set_loaded_len(0);
        let result = run_rotate_bws_token_with_stdin(
            RotateBwsTokenCommand { serial: Some(2001) },
            &mut failed,
        );
        assert!(result.is_err(), "verify failure should fail rotation");
        let failed_reports = failed.mock.reports();
        assert_eq!(
            failed_reports[0].checks.get(&CheckName::LocalStorage),
            Some(&CheckStatus::Failed)
        );
        Ok(())
    }

    #[test]
    fn rotate_stdin_reads_pin_only_when_required() -> Result<()> {
        let mut requires_pin = AppMockBoundary::new()
            .expect_rotation_success()
            .expect_pin();
        requires_pin.mock.set_primary_requires_pin(true);
        run_rotate_bws_token_with_stdin(
            RotateBwsTokenCommand { serial: Some(2001) },
            &mut requires_pin,
        )?;

        let mut no_pin = AppMockBoundary::new().expect_rotation_success();
        run_rotate_bws_token_with_stdin(RotateBwsTokenCommand { serial: Some(2001) }, &mut no_pin)
    }
}
