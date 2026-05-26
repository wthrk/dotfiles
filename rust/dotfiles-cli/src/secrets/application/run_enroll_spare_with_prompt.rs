use crate::Result;
use crate::secrets::{
    domain::{
        manifest::BootstrapSecretDocument,
        piv::SecretName,
        values::{EnrollSpareCommand, EnrollSummary},
    },
    ports::{self},
};

/// primary YubiKey から読み出した secret を prompt 運用の spare YubiKey へ複製する。
///
/// primary/spare 解決順序を固定して同一 serial への誤登録を防ぎ、secret 転送手段の詳細は
/// `SecretLoadPort` / `SecretStorePort` 境界へ閉じ込める。
pub(crate) fn run_enroll_spare_with_prompt<
    B: ports::DeviceSerialPort
        + ports::SpareDeviceSerialPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + ports::StorageSetupPort
        + ports::SecretLoadPort
        + ports::SecretStorePort
        + ports::StorageVerifyPort
        + ports::ReportPort,
>(
    command: EnrollSpareCommand,
    boundary: &mut B,
) -> Result<()> {
    let primary_serial = boundary.resolve_device_serial(command.primary_serial)?;
    let spare_serial =
        boundary.resolve_spare_device_serial(Some(primary_serial), command.spare_serial)?;
    boundary.setup_storage(spare_serial)?;
    let primary_pin = if boundary.device_requires_pin(primary_serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    let bw_email =
        boundary.load_secret(primary_serial, SecretName::BwEmail, primary_pin.as_ref())?;
    let bw_password =
        boundary.load_secret(primary_serial, SecretName::BwPassword, primary_pin.as_ref())?;
    let bws_access_token = boundary.load_secret(
        primary_serial,
        SecretName::BwsAccessToken,
        primary_pin.as_ref(),
    )?;
    let document = BootstrapSecretDocument::from_interactive_secrets(
        bw_email.as_ref(),
        bw_password.as_ref(),
        bws_access_token.as_ref(),
    )?;
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
    let spare_pin = if boundary.device_requires_pin(spare_serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    boundary.verify_local_storage(spare_serial, spare_pin.as_ref())?;
    boundary.write_enroll_report(&EnrollSummary::spare_completed(spare_serial))
}

#[cfg(test)]
mod tests {
    use crate::Result;
    use crate::secrets::{
        domain::{
            material::SecretMaterial,
            piv::SecretName,
            values::{EnrollSpareCommand, VerifySummary},
        },
        ports::{
            DevicePinPolicyPort, DeviceSerialPort, PinInputPort, ReportPort, SecretLoadPort,
            SecretStorePort, SpareDeviceSerialPort, StorageSetupPort, StorageVerifyPort,
        },
    };

    use super::run_enroll_spare_with_prompt;

    #[derive(Default)]
    struct FakeBoundary {
        resolved_primary: Option<u32>,
        resolved_spare: Option<(Option<u32>, Option<u32>)>,
        requires_pin_for: Vec<u32>,
        verify_received_pin: bool,
        load_received_pin: bool,
        fail_verify: bool,
    }

    impl DeviceSerialPort for FakeBoundary {
        fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
            self.resolved_primary = requested;
            Ok(requested.unwrap_or(2001))
        }
    }
    impl SpareDeviceSerialPort for FakeBoundary {
        fn resolve_spare_device_serial(
            &mut self,
            primary_serial: Option<u32>,
            requested_spare_serial: Option<u32>,
        ) -> Result<u32> {
            self.resolved_spare = Some((primary_serial, requested_spare_serial));
            Ok(requested_spare_serial.unwrap_or(2002))
        }
    }
    impl DevicePinPolicyPort for FakeBoundary {
        fn device_requires_pin(&mut self, serial: u32) -> Result<bool> {
            Ok(self.requires_pin_for.contains(&serial))
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
    impl SecretLoadPort for FakeBoundary {
        fn load_secret(
            &mut self,
            _serial: u32,
            _name: SecretName,
            pin: Option<&SecretMaterial>,
        ) -> Result<SecretMaterial> {
            self.load_received_pin = pin.is_some();
            Ok(SecretMaterial::from_vec(b"value".to_vec()))
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
    fn enroll_spare_prompt_resolves_primary_then_spare() {
        let mut boundary = FakeBoundary::default();
        let result = run_enroll_spare_with_prompt(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2002),
            },
            &mut boundary,
        );
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(boundary.resolved_primary, Some(2001));
        assert_eq!(boundary.resolved_spare, Some((Some(2001), Some(2002))));
    }

    #[test]
    fn enroll_spare_prompt_reads_pin_for_spare_verify_when_required() {
        let mut boundary = FakeBoundary {
            requires_pin_for: vec![2002],
            ..Default::default()
        };
        let result = run_enroll_spare_with_prompt(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2002),
            },
            &mut boundary,
        );
        assert!(result.is_ok(), "{result:?}");
        assert!(boundary.verify_received_pin);
        assert!(!boundary.load_received_pin);
    }

    #[test]
    fn enroll_spare_prompt_reads_primary_pin_when_required() {
        let mut boundary = FakeBoundary {
            requires_pin_for: vec![2001],
            ..Default::default()
        };
        let result = run_enroll_spare_with_prompt(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2002),
            },
            &mut boundary,
        );
        assert!(result.is_ok(), "{result:?}");
        assert!(boundary.load_received_pin);
        assert!(!boundary.verify_received_pin);
    }

    #[test]
    fn enroll_spare_prompt_stops_when_verify_fails() {
        let mut boundary = FakeBoundary {
            fail_verify: true,
            ..Default::default()
        };
        let result = run_enroll_spare_with_prompt(
            EnrollSpareCommand {
                primary_serial: Some(2001),
                spare_serial: Some(2002),
            },
            &mut boundary,
        );
        assert!(result.is_err(), "verify error should stop use case");
    }
}
