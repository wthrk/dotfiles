use crate::Result;
use crate::secrets::{
    domain::{
        piv::SecretName,
        values::{EnrollPrimaryCommand, EnrollSummary},
    },
    ports::{self},
};

/// stdin JSON document で primary YubiKey に bootstrap secret 一式を登録する。
///
/// JSON parse を use case へ持ち込まず `BootstrapSecretDocumentInputPort` へ委譲し、
/// enrollment 手順のみを application 層で固定する。
pub(crate) fn run_enroll_primary_with_stdin_json<
    B: ports::DeviceSerialPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + ports::StorageSetupPort
        + ports::BootstrapSecretDocumentInputPort
        + ports::SecretStorePort
        + ports::StorageVerifyPort
        + ports::ReportPort,
>(
    command: EnrollPrimaryCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = boundary.resolve_device_serial(command.serial)?;
    boundary.setup_storage(serial)?;
    let document = boundary.read_bootstrap_secret_document_noninteractive()?;
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
            manifest::BootstrapSecretDocument,
            material::SecretMaterial,
            values::{EnrollPrimaryCommand, VerifySummary},
        },
        ports::{
            BootstrapSecretDocumentInputPort, DevicePinPolicyPort, DeviceSerialPort, PinInputPort,
            ReportPort, SecretStorePort, StorageSetupPort, StorageVerifyPort,
        },
    };

    use super::run_enroll_primary_with_stdin_json;

    #[derive(Default)]
    struct FakeBoundary {
        requires_pin: bool,
        verify_received_pin: bool,
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
            _name: crate::secrets::domain::piv::SecretName,
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
    fn enroll_primary_stdin_json_reads_pin_only_when_required() {
        let mut boundary = FakeBoundary {
            requires_pin: true,
            ..Default::default()
        };
        let result = run_enroll_primary_with_stdin_json(
            EnrollPrimaryCommand { serial: Some(2001) },
            &mut boundary,
        );
        assert!(result.is_ok(), "{result:?}");
        assert!(boundary.verify_received_pin);
    }

    #[test]
    fn enroll_primary_stdin_json_skips_pin_when_not_required() {
        let mut boundary = FakeBoundary::default();
        let result = run_enroll_primary_with_stdin_json(
            EnrollPrimaryCommand { serial: Some(2001) },
            &mut boundary,
        );
        assert!(result.is_ok(), "{result:?}");
        assert!(!boundary.verify_received_pin);
    }

    #[test]
    fn enroll_primary_stdin_json_stops_when_verify_fails() {
        let mut boundary = FakeBoundary {
            fail_verify: true,
            ..Default::default()
        };
        let result = run_enroll_primary_with_stdin_json(
            EnrollPrimaryCommand { serial: Some(2001) },
            &mut boundary,
        );
        assert!(result.is_err(), "verify error should stop use case");
    }
}
