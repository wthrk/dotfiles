//! verify-yubikey の device 解決順序を固定し、外部検証の停止境界を曖昧化しない。

use crate::Result;
use crate::secrets::{
    domain::{
        material::SecretMaterial,
        piv::{SecretName, validate_piv_pin_len},
        storage::{SecretStorageReadIntent, SecretStorageVerificationPlan},
        values::{BwsSecretName, CheckName, CheckStatus, VerifySummary, VerifyYubikeyCommand},
    },
    ports::{self, BwsClientPort, SecretStoragePort},
};

/// 保存済み secret の存在と、要求された外部確認項目を検証する。
///
/// serial 未指定時の自動選択を device port 境界へ委譲し、local storage 検証を完了条件の
/// 先頭に固定する。BWS 検証は local storage 成功後に実行し、未実装の外部確認は
/// report 境界で通知して明示的に停止することで、verify 結果の責任範囲を曖昧にしない。
pub(crate) fn run_verify_yubikey_with<
    B: ports::DeviceSerialPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + SecretStoragePort
        + BwsClientPort
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
    let mut summary = VerifySummary::local_storage_verified(serial);
    let mut first_error: Option<anyhow::Error> = None;
    if requested.contains(&CheckName::Bws) {
        match run_bws_check(serial, pin.as_ref(), boundary) {
            Ok(()) => {
                summary.checks.insert(CheckName::Bws, CheckStatus::Ok);
            }
            Err(err) => {
                summary.checks.insert(CheckName::Bws, CheckStatus::Failed);
                first_error = Some(err);
            }
        }
    }
    let unimplemented: Vec<CheckName> = requested
        .iter()
        .copied()
        .filter(|c| *c != CheckName::Bws)
        .collect();
    if !unimplemented.is_empty() {
        for c in &unimplemented {
            summary.checks.insert(*c, CheckStatus::Failed);
        }
        boundary.write_verify_report(&summary)?;
        return Err(command.external_checks_unavailable_error(&unimplemented));
    }
    boundary.write_verify_report(&summary)?;
    first_error.map_or(Ok(()), Err)
}

/// YubiKey から bws-access-token を読み出し、両 BWS secret が取得できることを確認する。
fn run_bws_check<B: SecretStoragePort + BwsClientPort>(
    serial: u32,
    pin: Option<&SecretMaterial>,
    boundary: &mut B,
) -> Result<()> {
    let token_storage = SecretName::BwsAccessToken.storage_spec(serial);
    let token_inspection = boundary.inspect_secret_storage_read(serial, &token_storage)?;
    let token_intent =
        SecretStorageReadIntent::from_inspection(token_storage, token_inspection)?;
    let token = boundary
        .load_secret(serial, &token_intent, pin)
        .map_err(|error| token_intent.decode_error(error))?;
    token_intent.validate_loaded_secret(&token)?;
    for name in [
        BwsSecretName::GpgSecretKeyBackup,
        BwsSecretName::PasswordStoreRemote,
    ] {
        boundary.fetch_bws_secret(&token, name)?;
    }
    Ok(())
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
    fn verify_runs_bws_check_when_requested() -> Result<()> {
        let mut boundary = AppMockBoundary::new().expect_report();
        run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: Some(2001),
                checks: vec![ExternalCheck::Bws],
                all: false,
            },
            &mut boundary,
        )
    }

    #[test]
    fn verify_reports_bws_failed_when_fetch_fails() {
        let mut boundary = AppMockBoundary::new().expect_report();
        boundary.mock.set_bws_error("mock BWS fetch failed");
        let err = run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: Some(2001),
                checks: vec![ExternalCheck::Bws],
                all: false,
            },
            &mut boundary,
        )
        .expect_err("BWS fetch failure should propagate as error");

        assert!(
            err.to_string().contains("mock BWS fetch failed"),
            "unexpected error: {err:#}"
        );
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

    #[test]
    fn verify_still_unavailable_for_bw_login() {
        let mut boundary = AppMockBoundary::new().expect_report();
        let err = run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: Some(2001),
                checks: vec![ExternalCheck::BwLogin],
                all: false,
            },
            &mut boundary,
        )
        .expect_err("bw-login check should remain unimplemented");

        assert!(
            err.to_string()
                .contains("external checks are not implemented yet"),
            "unexpected error: {err:#}"
        );
    }
}
