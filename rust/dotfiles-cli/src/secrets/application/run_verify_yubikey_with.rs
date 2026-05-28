//! verify-yubikey の device 解決順序を固定し、未実装外部検証の停止境界を曖昧化しない。

use crate::Result;
use crate::secrets::{
    domain::{
        piv::validate_piv_pin_len,
        storage::{SecretStorageReadIntent, SecretStorageVerificationPlan},
        values::{VerifySummary, VerifyYubikeyCommand},
    },
    ports::{self, SecretStoragePort},
};

/// 保存済み secret の存在と、要求された外部確認項目を検証する。
///
/// serial 未指定時の自動選択を device port 境界へ委譲し、local storage 検証を完了条件の
/// 先頭に固定する。未実装の外部確認は report 境界で通知して明示的に停止することで、
/// verify 結果の責任範囲を曖昧にしない。
pub(crate) fn run_verify_yubikey_with<
    B: ports::DeviceSerialPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + SecretStoragePort
        + ports::ReportPort,
>(
    command: VerifyYubikeyCommand,
    boundary: &mut B,
) -> Result<()> {
    let requested = command.requested_external_checks()?;
    let serial = boundary.resolve_device_serial(command.serial)?;
    let pin = if boundary.device_requires_pin(serial)? {
        let pin = boundary.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };
    let local_verify: Result<()> = (|| {
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
    if let Err(err) = local_verify {
        return boundary
            .write_verify_report(&VerifySummary::local_storage_failed(serial))
            .and(Err(err));
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

#[cfg(test)]
mod tests {
    use crate::Result;
    use crate::secrets::{
        application::app_test_support::AppMockBoundary,
        domain::values::{CheckName, CheckStatus, ExternalCheck, VerifyYubikeyCommand},
    };

    use super::run_verify_yubikey_with;

    #[test]
    fn verify_requests_pin_when_required() -> Result<()> {
        let mut boundary = AppMockBoundary::new().expect_report().expect_pin();
        boundary.mock.set_primary_requires_pin(true);
        run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: None,
                checks: Vec::new(),
                all: false,
            },
            &mut boundary,
        )
    }

    #[test]
    fn verify_stops_when_external_checks_requested() -> Result<()> {
        let mut boundary = AppMockBoundary::new().expect_report();
        let err = run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: Some(2001),
                checks: vec![ExternalCheck::Bws],
                all: false,
            },
            &mut boundary,
        )
        .expect_err("external checks should fail in current phase");

        assert!(
            err.to_string()
                .contains("external checks are not implemented yet")
        );
        Ok(())
    }

    #[test]
    fn verify_rejects_conflicting_external_checks_before_device_resolution() {
        let mut boundary = AppMockBoundary::new();
        boundary.mock.set_primary_available(false);
        let err = run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: None,
                checks: vec![ExternalCheck::Bws],
                all: true,
            },
            &mut boundary,
        )
        .expect_err("conflicting check options should fail before device resolution");

        assert!(
            err.to_string()
                .contains("--all and --check cannot be used together"),
            "unexpected error: {err:#}"
        );
        assert!(
            boundary.mock.resolution_order().is_empty(),
            "device resolution must not run after input precondition failure"
        );
    }

    #[test]
    fn verify_reports_failed_summary_when_local_storage_is_invalid() {
        let mut boundary = AppMockBoundary::new().expect_report();
        boundary.mock.set_loaded_len(0);
        let err = run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: Some(2001),
                checks: Vec::new(),
                all: false,
            },
            &mut boundary,
        )
        .expect_err("invalid storage should fail verify");

        assert!(
            err.to_string().contains("bw-email must not be empty"),
            "unexpected error: {err:#}"
        );
        let reports = boundary.mock.reports();
        assert_eq!(
            reports[0].checks.get(&CheckName::LocalStorage),
            Some(&CheckStatus::Failed)
        );
    }
}
