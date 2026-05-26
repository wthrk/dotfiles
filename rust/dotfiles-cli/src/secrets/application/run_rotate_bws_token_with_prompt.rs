use anyhow::bail;

use crate::Result;
use crate::secrets::{
    domain::{
        piv::SecretName,
        values::{RotateBwsTokenCommand, VerifySummary},
    },
    ports::{self},
};

const NONINTERACTIVE_SERIAL_ERROR: &str = "pass --serial in non-interactive use";

/// prompt 入力で BWS token を更新し、YubiKey 保存状態を再検証する。
///
/// serial 未指定時は非対話運用の誤書き込みを防ぐため停止し、保存失敗と検証失敗の責務は
/// `SecretStorePort` / `StorageVerifyPort` の境界へ分離する。
pub(crate) fn run_rotate_bws_token_with_prompt<
    B: ports::SecretStorePort
        + ports::SecretInputPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + ports::StorageVerifyPort
        + ports::ReportPort,
>(
    command: RotateBwsTokenCommand,
    boundary: &mut B,
) -> Result<()> {
    let Some(serial) = command.serial else {
        bail!(NONINTERACTIVE_SERIAL_ERROR);
    };
    let token = boundary.read_hidden_secret(SecretName::BwsAccessToken)?;
    boundary.store_secret(serial, SecretName::BwsAccessToken, true, &token)?;
    let pin = if boundary.device_requires_pin(serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    match boundary.verify_local_storage(serial, pin.as_ref()) {
        Ok(()) => boundary.write_verify_report(&VerifySummary::local_storage_verified(serial)),
        Err(err) => boundary
            .write_verify_report(&VerifySummary::local_storage_failed(serial))
            .and(Err(err)),
    }
}

#[cfg(test)]
mod tests {
    use super::run_rotate_bws_token_with_prompt;
    use crate::Result;
    use crate::secrets::{
        domain::{
            material::SecretMaterial,
            piv::SecretName,
            values::{CheckName, CheckStatus, EnrollSummary, RotateBwsTokenCommand, VerifySummary},
        },
        ports::{
            DevicePinPolicyPort, PinInputPort, ReportPort, SecretInputPort, SecretStorePort,
            StorageVerifyPort,
        },
    };
    use std::cell::RefCell;

    #[derive(Default)]
    struct FakeBoundary {
        fail_verify: bool,
        requires_pin: bool,
        verify_received_pin: bool,
        reports: RefCell<Vec<VerifySummary>>,
        store_calls: usize,
    }

    impl SecretInputPort for FakeBoundary {
        fn read_visible_secret(&self) -> Result<SecretMaterial> {
            unreachable!()
        }
        fn read_hidden_secret(&self, _name: SecretName) -> Result<SecretMaterial> {
            Ok(SecretMaterial::from_vec(b"token".to_vec()))
        }
        fn read_stdin_secret(&self) -> Result<SecretMaterial> {
            unreachable!()
        }
    }
    impl SecretStorePort for FakeBoundary {
        fn store_secret(
            &mut self,
            _serial: u32,
            _name: SecretName,
            _force: bool,
            _secret: &SecretMaterial,
        ) -> Result<()> {
            self.store_calls += 1;
            Ok(())
        }
    }
    impl DevicePinPolicyPort for FakeBoundary {
        fn device_requires_pin(&mut self, _serial: u32) -> Result<bool> {
            Ok(self.requires_pin)
        }
    }
    impl PinInputPort for FakeBoundary {
        fn read_pin(&self) -> Result<SecretMaterial> {
            Ok(SecretMaterial::from_vec(b"123456".to_vec()))
        }
    }
    impl StorageVerifyPort for FakeBoundary {
        fn verify_local_storage(
            &mut self,
            _serial: u32,
            pin: Option<&SecretMaterial>,
        ) -> Result<()> {
            self.verify_received_pin = pin.is_some();
            if self.fail_verify {
                return Err(std::io::Error::other("verify failed").into());
            }
            Ok(())
        }
    }
    impl ReportPort for FakeBoundary {
        fn write_enroll_report(&self, _summary: &EnrollSummary) -> Result<()> {
            unreachable!()
        }
        fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
            self.reports.borrow_mut().push(summary.clone());
            Ok(())
        }
    }

    #[test]
    fn rotate_prompt_stops_without_serial() {
        let mut boundary = FakeBoundary::default();
        let result =
            run_rotate_bws_token_with_prompt(RotateBwsTokenCommand { serial: None }, &mut boundary);
        assert!(result.is_err());
        assert_eq!(boundary.store_calls, 0);
    }

    #[test]
    fn rotate_prompt_reports_verify_success_and_failure() {
        let mut success = FakeBoundary::default();
        let ok = run_rotate_bws_token_with_prompt(
            RotateBwsTokenCommand { serial: Some(2001) },
            &mut success,
        );
        assert!(ok.is_ok(), "{ok:?}");
        let success_reports = success.reports.borrow();
        assert_eq!(success_reports.len(), 1);
        assert_eq!(
            success_reports[0].checks.get(&CheckName::LocalStorage),
            Some(&CheckStatus::Ok)
        );

        let mut failed = FakeBoundary {
            fail_verify: true,
            ..Default::default()
        };
        let err = run_rotate_bws_token_with_prompt(
            RotateBwsTokenCommand { serial: Some(2001) },
            &mut failed,
        );
        assert!(err.is_err());
        let failed_reports = failed.reports.borrow();
        assert_eq!(failed_reports.len(), 1);
        assert_eq!(
            failed_reports[0].checks.get(&CheckName::LocalStorage),
            Some(&CheckStatus::Failed)
        );
    }

    #[test]
    fn rotate_prompt_reads_pin_only_when_required() {
        let mut requires_pin = FakeBoundary {
            requires_pin: true,
            ..Default::default()
        };
        let result = run_rotate_bws_token_with_prompt(
            RotateBwsTokenCommand { serial: Some(2001) },
            &mut requires_pin,
        );
        assert!(result.is_ok(), "{result:?}");
        assert!(requires_pin.verify_received_pin);

        let mut no_pin = FakeBoundary::default();
        let result = run_rotate_bws_token_with_prompt(
            RotateBwsTokenCommand { serial: Some(2001) },
            &mut no_pin,
        );
        assert!(result.is_ok(), "{result:?}");
        assert!(!no_pin.verify_received_pin);
    }
}
