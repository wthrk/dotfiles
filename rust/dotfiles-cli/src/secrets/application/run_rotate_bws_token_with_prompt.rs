//! rotate-bws-token(prompt) の順序を固定し、更新手順と検証手順の責任境界を崩さない。

use std::collections::BTreeSet;

use anyhow::bail;

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

/// prompt 入力で BWS token を更新し、YubiKey 保存状態を再検証する。
///
/// serial 未指定時は port 境界で対象 device を解決し、token 入力前に既存 local storage を
/// read/validate する。更新不能な状態では new token を受け取らない。
pub(crate) fn run_rotate_bws_token_with_prompt<
    B: ports::DeviceSerialPort
        + ports::SecretInputPort
        + ports::RotationContinuationPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + SecretStoragePort
        + ports::ReportPort,
>(
    command: RotateBwsTokenCommand,
    boundary: &mut B,
) -> Result<()> {
    let mut updated_serials = BTreeSet::new();
    let mut next_requested_serial = command.serial;
    let mut token = None;

    loop {
        let serial = boundary.resolve_device_serial(next_requested_serial)?;
        if !updated_serials.insert(serial) {
            bail!("selected YubiKey was already updated");
        }

        let storage = command.storage_spec(serial);
        let inspection = boundary.inspect_secret_storage_write(serial, &storage)?;
        SecretStorageWriteIntent::ensure_store_preconditions(&inspection)?;
        let pin = if boundary.device_requires_pin(serial)? {
            let pin = boundary.read_pin()?;
            validate_piv_pin_len(pin.len())?;
            Some(pin)
        } else {
            None
        };
        let pre_update_verify: Result<()> = (|| {
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
        if let Err(err) = pre_update_verify {
            return boundary
                .write_verify_report(&VerifySummary::local_storage_failed(serial))
                .and(Err(err));
        }

        if token.is_none() {
            token = Some(boundary.read_bws_access_token_secret()?);
        }
        let Some(token) = token.as_ref() else {
            bail!("rotate token is unavailable");
        };
        let intent = SecretStorageWriteIntent::store(storage, inspection, token.len())?;
        boundary.store_secret(serial, intent, token)?;
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
            Ok(()) => {
                boundary.write_verify_report(&VerifySummary::local_storage_verified(serial))?
            }
            Err(err) => {
                return boundary
                    .write_verify_report(&VerifySummary::local_storage_failed(serial))
                    .and(Err(err));
            }
        }

        if command.serial.is_some() || !boundary.continue_rotation()? {
            return Ok(());
        }
        next_requested_serial = None;
    }
}

#[cfg(all(test, feature = "secrets-internal-test-stub"))]
mod tests {
    use crate::Result;
    use crate::secrets::{
        application::app_test_support::AppMockBoundary,
        domain::{
            piv::SecretName,
            values::{CheckName, CheckStatus, RotateBwsTokenCommand},
        },
    };

    use super::run_rotate_bws_token_with_prompt;

    #[test]
    fn rotate_prompt_resolves_serial_when_omitted() -> Result<()> {
        let mut boundary = AppMockBoundary::new();
        run_rotate_bws_token_with_prompt(RotateBwsTokenCommand { serial: None }, &mut boundary)?;
        assert_eq!(boundary.mock.stores().len(), 1);
        Ok(())
    }

    #[test]
    fn rotate_prompt_checks_storage_before_reading_token() {
        let mut boundary = AppMockBoundary::new();
        boundary.mock.set_write_manifest_missing();
        boundary.mock.set_secret_error(
            SecretName::BwsAccessToken,
            "token should not be read before preflight",
        );

        let result = run_rotate_bws_token_with_prompt(
            RotateBwsTokenCommand { serial: Some(2001) },
            &mut boundary,
        );

        assert!(
            result.is_err(),
            "storage preflight should stop before token read"
        );
    }

    #[test]
    fn rotate_prompt_reports_verify_success() -> Result<()> {
        let mut success = AppMockBoundary::new().expect_rotation_success();
        run_rotate_bws_token_with_prompt(
            RotateBwsTokenCommand { serial: Some(2001) },
            &mut success,
        )?;
        let success_reports = success.mock.reports();
        assert_eq!(
            success_reports[0].checks.get(&CheckName::LocalStorage),
            Some(&CheckStatus::Ok)
        );
        Ok(())
    }

    #[test]
    fn rotate_prompt_stops_before_token_when_existing_storage_is_invalid() {
        let mut failed = AppMockBoundary::new().expect_report();
        failed.mock.set_loaded_len(0);
        failed
            .mock
            .set_secret_error(SecretName::BwsAccessToken, "token must not be read");
        let result = run_rotate_bws_token_with_prompt(
            RotateBwsTokenCommand { serial: Some(2001) },
            &mut failed,
        );
        let err = result.expect_err("invalid existing storage should fail before token read");
        assert!(
            err.to_string().contains("bw-email must not be empty"),
            "unexpected error: {err:#}"
        );
        assert!(
            failed.mock.stores().is_empty(),
            "invalid existing storage must stop before store"
        );
        let failed_reports = failed.mock.reports();
        assert_eq!(
            failed_reports[0].checks.get(&CheckName::LocalStorage),
            Some(&CheckStatus::Failed)
        );
    }

    #[test]
    fn rotate_prompt_reads_pin_only_when_required() -> Result<()> {
        let mut requires_pin = AppMockBoundary::new()
            .expect_rotation_success()
            .expect_pin();
        requires_pin.mock.set_primary_requires_pin(true);
        run_rotate_bws_token_with_prompt(
            RotateBwsTokenCommand { serial: Some(2001) },
            &mut requires_pin,
        )?;

        let mut no_pin = AppMockBoundary::new().expect_rotation_success();
        run_rotate_bws_token_with_prompt(RotateBwsTokenCommand { serial: Some(2001) }, &mut no_pin)
    }

    #[test]
    fn rotate_prompt_can_continue_to_another_interactive_device() -> Result<()> {
        let mut boundary = AppMockBoundary::new()
            .expect_store_times(2)
            .expect_report_times(2);
        boundary
            .mock
            .set_device_resolution_sequence(vec![2001, 2002]);
        boundary.mock.set_rotation_continuations(vec![true, false]);

        run_rotate_bws_token_with_prompt(RotateBwsTokenCommand { serial: None }, &mut boundary)?;

        assert_eq!(boundary.mock.reports()[0].serial, 2001);
        assert_eq!(boundary.mock.reports()[1].serial, 2002);
        assert_eq!(boundary.mock.stores().len(), 2);
        Ok(())
    }

    #[test]
    fn rotate_prompt_rejects_continued_selection_of_updated_device() -> Result<()> {
        let mut boundary = AppMockBoundary::new().expect_store_times(1).expect_report();
        boundary
            .mock
            .set_device_resolution_sequence(vec![2001, 2001]);
        boundary.mock.set_rotation_continuations(vec![true]);

        let err =
            run_rotate_bws_token_with_prompt(RotateBwsTokenCommand { serial: None }, &mut boundary)
                .expect_err("continued rotate accepted an already updated device");

        assert_eq!(err.to_string(), "selected YubiKey was already updated");
        assert_eq!(boundary.mock.stores().len(), 1);
        Ok(())
    }
}
