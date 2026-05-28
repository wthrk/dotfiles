//! verify-yubikey の device 解決順序を固定し、外部検証の責務境界を application に維持する。

use crate::Result;
use crate::secrets::{
    domain::{
        piv::validate_piv_pin_len,
        storage::{SecretStorageReadIntent, SecretStorageVerificationPlan},
        values::{BwsSecretName, CheckName, CheckStatus, VerifySummary, VerifyYubikeyCommand},
    },
    ports::{self, SecretStoragePort},
};

/// 保存済み secret の存在と、要求された外部確認項目を検証する。
///
/// serial 未指定時の自動選択を device port 境界へ委譲し、local storage 検証を完了条件の
/// 先頭に固定する。外部確認結果は report 境界へ明示的に反映し、verify 結果の責任範囲を
/// 曖昧にしない。
pub(crate) fn run_verify_yubikey_with<
    B: ports::DeviceSerialPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + SecretStoragePort
        + ports::ReportPort
        + ports::BwsClientPort,
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
    let local_verify: Result<Option<crate::secrets::domain::material::SecretMaterial>> = (|| {
        let mut bws_access_token = None;
        for storage in SecretStorageVerificationPlan::for_serial(serial).into_targets() {
            let is_bws_access_token =
                storage.name == crate::secrets::domain::piv::SecretName::BwsAccessToken;
            let inspection = boundary.inspect_secret_storage_read(serial, &storage)?;
            let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
            let secret = boundary
                .load_secret(serial, &intent, pin.as_ref())
                .map_err(|error| intent.decode_error(error))?;
            intent.validate_loaded_secret(&secret)?;
            if is_bws_access_token {
                bws_access_token = Some(secret);
            }
        }
        Ok(bws_access_token)
    })();
    let bws_access_token = match local_verify {
        Ok(value) => value,
        Err(err) => {
            return boundary
                .write_verify_report(&VerifySummary::local_storage_failed(serial))
                .and(Err(err));
        }
    };
    if requested.is_empty() {
        return boundary.write_verify_report(&VerifySummary::local_storage_verified(serial));
    }
    let access_token = bws_access_token.ok_or_else(|| {
        anyhow::anyhow!(
            "internal invariant violated: verification plan did not yield bws-access-token"
        )
    })?;
    let mut summary = VerifySummary::local_storage_verified(serial);
    let mut first_error = None;
    for check in requested {
        match check {
            CheckName::Bws => {
                let result = boundary
                    .fetch_bws_secret(&access_token, BwsSecretName::GpgSecretKeyBackup)
                    .and_then(|_| {
                        boundary.fetch_bws_secret(&access_token, BwsSecretName::PasswordStoreRemote)
                    });
                match result {
                    Ok(_) => summary.mark_external_check(CheckName::Bws, CheckStatus::Ok),
                    Err(error) => {
                        summary.mark_external_check(CheckName::Bws, CheckStatus::Failed);
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                    }
                }
            }
            CheckName::BwLogin => {
                summary.mark_external_check(CheckName::BwLogin, CheckStatus::Failed);
                if first_error.is_none() {
                    first_error = Some(anyhow::anyhow!(
                        "external checks are not implemented yet: {}",
                        CheckName::BwLogin.as_str()
                    ));
                }
            }
            CheckName::Setup
            | CheckName::BwEmail
            | CheckName::BwPassword
            | CheckName::BwsAccessToken
            | CheckName::LocalStorage => {
                unreachable!("requested_external_checks returned a non-external verification check")
            }
        }
    }
    boundary.write_verify_report(&summary)?;
    if let Some(error) = first_error {
        return Err(error);
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
    fn verify_executes_bws_external_check_when_requested() -> Result<()> {
        let mut boundary = AppMockBoundary::new().expect_report();
        run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: Some(2001),
                checks: vec![ExternalCheck::Bws],
                all: false,
            },
            &mut boundary,
        )?;

        assert_eq!(
            boundary.mock.bws_fetches(),
            vec![
                crate::secrets::domain::values::BwsSecretName::GpgSecretKeyBackup,
                crate::secrets::domain::values::BwsSecretName::PasswordStoreRemote
            ]
        );
        let reports = boundary.mock.reports();
        assert_eq!(
            reports[0].checks.get(&CheckName::Bws),
            Some(&CheckStatus::Ok)
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
