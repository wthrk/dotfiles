use crate::Result;
use crate::secrets::{
    domain::{
        manifest::BootstrapSecretDocument,
        piv::SecretName,
        values::{EnrollPrimaryCommand, EnrollSummary},
    },
    ports::{self},
};

/// prompt 入力で primary YubiKey に bootstrap secret 一式を登録する。
///
/// 入力手段の詳細は `SecretInputPort` 側へ閉じ込め、use case は setup→store→verify の
/// 順序制御だけを担って application 層の責務境界を維持する。
pub(crate) fn run_enroll_primary_with_prompt<
    B: ports::DeviceSerialPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + ports::StorageSetupPort
        + ports::SecretInputPort
        + ports::SecretStorePort
        + ports::StorageVerifyPort
        + ports::ReportPort,
>(
    command: EnrollPrimaryCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = boundary.resolve_device_serial(command.serial)?;
    boundary.setup_storage(serial)?;
    let bw_email = boundary.read_visible_secret()?;
    let bw_password = boundary.read_hidden_secret(SecretName::BwPassword)?;
    let bws_access_token = boundary.read_hidden_secret(SecretName::BwsAccessToken)?;
    let document = BootstrapSecretDocument::from_interactive_secrets(
        bw_email.as_ref(),
        bw_password.as_ref(),
        bws_access_token.as_ref(),
    )?;
    boundary.store_secret(serial, SecretName::BwEmail, false, &document.bw_email)?;
    boundary.store_secret(serial, SecretName::BwPassword, false, &document.bw_password)?;
    boundary.store_secret(
        serial,
        SecretName::BwsAccessToken,
        false,
        &document.bws_access_token,
    )?;
    let pin = if boundary.device_requires_pin(serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    boundary.verify_local_storage(serial, pin.as_ref())?;
    boundary.write_enroll_report(&EnrollSummary::primary_completed(serial))
}

#[cfg(test)]
mod tests {
    use crate::Result;
    use crate::secrets::{
        domain::{
            material::SecretMaterial,
            piv::SecretName,
            values::{CheckName, CheckStatus, EnrollPrimaryCommand, EnrollSummary, VerifySummary},
        },
        ports::{
            DevicePinPolicyPort, DeviceSerialPort, PinInputPort, ReportPort, SecretInputPort,
            SecretStorePort, StorageSetupPort, StorageVerifyPort,
        },
    };

    use super::run_enroll_primary_with_prompt;

    #[derive(Default)]
    struct FakeBoundary {
        stores: Vec<SecretName>,
        requires_pin: bool,
        verify_received_pin: bool,
        fail_setup: bool,
        fail_on_store: Option<SecretName>,
        fail_verify: bool,
    }

    impl DeviceSerialPort for FakeBoundary {
        fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
            Ok(requested.unwrap_or(2001))
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

    impl StorageSetupPort for FakeBoundary {
        fn setup_storage(&mut self, _serial: u32) -> Result<()> {
            if self.fail_setup {
                return Err(std::io::Error::other("setup failed").into());
            }
            Ok(())
        }
    }

    impl SecretInputPort for FakeBoundary {
        fn read_visible_secret(&self) -> Result<SecretMaterial> {
            Ok(SecretMaterial::from_vec(b"u@example.com".to_vec()))
        }

        fn read_hidden_secret(&self, _name: SecretName) -> Result<SecretMaterial> {
            Ok(SecretMaterial::from_vec(b"secret".to_vec()))
        }

        fn read_stdin_secret(&self) -> Result<SecretMaterial> {
            unreachable!("stdin path is not used in this use case")
        }
    }

    impl SecretStorePort for FakeBoundary {
        fn store_secret(
            &mut self,
            _serial: u32,
            name: SecretName,
            _force: bool,
            _secret: &SecretMaterial,
        ) -> Result<()> {
            if self.fail_on_store == Some(name) {
                return Err(std::io::Error::other("store failed").into());
            }
            self.stores.push(name);
            Ok(())
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
        fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()> {
            assert_eq!(summary.serial, 2001);
            assert_eq!(
                summary.checks.get(&CheckName::LocalStorage),
                Some(&CheckStatus::Ok)
            );
            Ok(())
        }

        fn write_verify_report(&self, _summary: &VerifySummary) -> Result<()> {
            unreachable!("verify report is not used in this use case")
        }
    }

    #[test]
    fn enroll_primary_prompt_path_stores_all_required_secrets() {
        let mut boundary = FakeBoundary::default();
        let result = run_enroll_primary_with_prompt(
            EnrollPrimaryCommand { serial: Some(2001) },
            &mut boundary,
        );
        assert!(result.is_ok(), "prompt path should succeed: {result:?}");
        assert_eq!(
            boundary.stores,
            vec![
                SecretName::BwEmail,
                SecretName::BwPassword,
                SecretName::BwsAccessToken
            ]
        );
        assert!(!boundary.verify_received_pin);
    }

    #[test]
    fn enroll_primary_reads_pin_when_device_requires_it() {
        let mut boundary = FakeBoundary {
            requires_pin: true,
            ..Default::default()
        };
        let result = run_enroll_primary_with_prompt(
            EnrollPrimaryCommand { serial: Some(2001) },
            &mut boundary,
        );
        assert!(result.is_ok(), "prompt path should succeed: {result:?}");
        assert!(boundary.verify_received_pin);
    }

    #[test]
    fn enroll_primary_stops_when_setup_fails() {
        let mut boundary = FakeBoundary {
            fail_setup: true,
            ..Default::default()
        };
        let result = run_enroll_primary_with_prompt(
            EnrollPrimaryCommand { serial: Some(2001) },
            &mut boundary,
        );
        assert!(result.is_err(), "setup error should stop use case");
        assert!(
            boundary.stores.is_empty(),
            "store should not run after setup error"
        );
    }

    #[test]
    fn enroll_primary_stops_when_secret_store_fails() {
        let mut boundary = FakeBoundary {
            fail_on_store: Some(SecretName::BwPassword),
            ..Default::default()
        };
        let result = run_enroll_primary_with_prompt(
            EnrollPrimaryCommand { serial: Some(2001) },
            &mut boundary,
        );
        assert!(result.is_err(), "store failure should stop use case");
        assert_eq!(boundary.stores, vec![SecretName::BwEmail]);
    }

    #[test]
    fn enroll_primary_stops_when_verify_fails() {
        let mut boundary = FakeBoundary {
            fail_verify: true,
            ..Default::default()
        };
        let result = run_enroll_primary_with_prompt(
            EnrollPrimaryCommand { serial: Some(2001) },
            &mut boundary,
        );
        assert!(result.is_err(), "verify error should stop use case");
    }
}
