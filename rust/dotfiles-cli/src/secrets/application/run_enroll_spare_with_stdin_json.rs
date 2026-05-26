use crate::Result;
use crate::secrets::{
    domain::{
        piv::SecretName,
        values::{EnrollSpareCommand, EnrollSummary},
    },
    ports::{self},
};

/// stdin JSON document で spare YubiKey に bootstrap secret 一式を登録する。
///
/// primary と spare の衝突停止条件を先に評価し、device 選択・入力実装は port に委譲して
/// use case の順序責務だけを保持する。
pub(crate) fn run_enroll_spare_with_stdin_json<
    B: ports::SpareDeviceSerialPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + ports::StorageSetupPort
        + ports::BootstrapSecretDocumentInputPort
        + ports::SecretStorePort
        + ports::StorageVerifyPort
        + ports::ReportPort,
>(
    command: EnrollSpareCommand,
    boundary: &mut B,
) -> Result<()> {
    let spare_serial =
        boundary.resolve_spare_device_serial(command.primary_serial, command.spare_serial)?;
    boundary.setup_storage(spare_serial)?;
    let document = boundary.read_bootstrap_secret_document_noninteractive()?;
    boundary.store_secret(spare_serial, SecretName::BwEmail, false, &document.bw_email)?;
    boundary.store_secret(
        spare_serial,
        SecretName::BwPassword,
        false,
        &document.bw_password,
    )?;
    boundary.store_secret(
        spare_serial,
        SecretName::BwsAccessToken,
        false,
        &document.bws_access_token,
    )?;
    let pin = if boundary.device_requires_pin(spare_serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    boundary.verify_local_storage(spare_serial, pin.as_ref())?;
    boundary.write_enroll_report(&EnrollSummary::spare_completed(spare_serial))
}

#[cfg(test)]
mod tests {
    use crate::Result;
    use crate::secrets::{
        domain::{
            manifest::BootstrapSecretDocument,
            material::SecretMaterial,
            piv::SecretName,
            values::{EnrollSpareCommand, VerifySummary},
        },
        ports::{
            BootstrapSecretDocumentInputPort, DevicePinPolicyPort, PinInputPort, ReportPort,
            SecretStorePort, SpareDeviceSerialPort, StorageSetupPort, StorageVerifyPort,
        },
    };

    use super::run_enroll_spare_with_stdin_json;

    #[derive(Default)]
    struct FakeBoundary {
        spare_resolution_args: Option<(Option<u32>, Option<u32>)>,
        requires_pin: bool,
        verify_received_pin: bool,
        fail_verify: bool,
    }

    impl SpareDeviceSerialPort for FakeBoundary {
        fn resolve_spare_device_serial(
            &mut self,
            primary_serial: Option<u32>,
            requested_spare_serial: Option<u32>,
        ) -> Result<u32> {
            self.spare_resolution_args = Some((primary_serial, requested_spare_serial));
            Ok(requested_spare_serial.unwrap_or(2002))
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
            Ok(())
        }
    }
    impl BootstrapSecretDocumentInputPort for FakeBoundary {
        fn read_bootstrap_secret_document_noninteractive(&self) -> Result<BootstrapSecretDocument> {
            BootstrapSecretDocument::from_interactive_secrets(b"e", b"p", b"t")
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
        fn write_enroll_report(
            &self,
            _summary: &crate::secrets::domain::values::EnrollSummary,
        ) -> Result<()> {
            Ok(())
        }
        fn write_verify_report(&self, _summary: &VerifySummary) -> Result<()> {
            unreachable!()
        }
    }

    #[test]
    fn enroll_spare_stdin_json_passes_primary_serial_to_spare_resolution() {
        let mut boundary = FakeBoundary::default();
        let result = run_enroll_spare_with_stdin_json(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2002),
            },
            &mut boundary,
        );
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(
            boundary.spare_resolution_args,
            Some((Some(2001), Some(2002)))
        );
    }

    #[test]
    fn enroll_spare_stdin_json_reads_pin_for_verify_when_required() {
        let mut boundary = FakeBoundary {
            requires_pin: true,
            ..Default::default()
        };
        let result = run_enroll_spare_with_stdin_json(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2002),
            },
            &mut boundary,
        );
        assert!(result.is_ok(), "{result:?}");
        assert!(boundary.verify_received_pin);
    }

    #[test]
    fn enroll_spare_stdin_json_skips_pin_when_not_required() {
        let mut boundary = FakeBoundary::default();
        let result = run_enroll_spare_with_stdin_json(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2002),
            },
            &mut boundary,
        );
        assert!(result.is_ok(), "{result:?}");
        assert!(!boundary.verify_received_pin);
    }

    #[test]
    fn enroll_spare_stdin_json_stops_when_verify_fails() {
        let mut boundary = FakeBoundary {
            fail_verify: true,
            ..Default::default()
        };
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
