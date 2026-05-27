//! `dotfiles secrets` の application 層。
//!
//! 個別 use case の orchestration を提供し、command 選択は entrypoint 側が担う。

pub(crate) mod run_enroll_primary_with_prompt;
pub(crate) mod run_enroll_primary_with_stdin_json;
pub(crate) mod run_enroll_spare_with_prompt;
pub(crate) mod run_enroll_spare_with_stdin_json;
pub(crate) mod run_get_with;
pub(crate) mod run_put_with_prompt;
pub(crate) mod run_put_with_stdin;
pub(crate) mod run_rotate_bws_token_with_prompt;
pub(crate) mod run_rotate_bws_token_with_stdin;
pub(crate) mod run_setup_with;
pub(crate) mod run_verify_yubikey_with;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::Result;
    use crate::secrets::domain::manifest::SecretManifest;
    use crate::secrets::domain::material::SecretMaterial;
    use crate::secrets::domain::piv::{PivApplicationVersion, SecretName, SecretStorageSpec};
    use crate::secrets::domain::storage::{
        SecretStorageReadInspection, SecretStorageReadIntent, SecretStorageSetupInspection,
        SecretStorageSetupProbe, SecretStorageWriteInspection, SecretStorageWriteIntent,
    };
    use crate::secrets::domain::values::{
        EnrollPrimaryCommand, EnrollSpareCommand, PutCommand, RotateBwsTokenCommand, SetupCommand,
    };
    use crate::secrets::ports;

    struct Boundary {
        _server: mockito::ServerGuard,
        device_serial: Option<u32>,
        spare_serial: Option<u32>,
        pin_error: Option<&'static str>,
        stdin_json_error: Option<&'static str>,
    }

    impl Boundary {
        fn new(device_serial: Option<u32>, spare_serial: Option<u32>) -> Self {
            Self {
                _server: mockito::Server::new(),
                device_serial,
                spare_serial,
                pin_error: None,
                stdin_json_error: None,
            }
        }

        fn with_pin_error(mut self, pin_error: &'static str) -> Self {
            self.pin_error = Some(pin_error);
            self
        }

        fn with_stdin_json_error(mut self, stdin_json_error: &'static str) -> Self {
            self.stdin_json_error = Some(stdin_json_error);
            self
        }
    }

    impl ports::DeviceSerialPort for Boundary {
        fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
            requested
                .or(self.device_serial)
                .ok_or_else(|| anyhow::anyhow!("pass --serial in non-interactive use"))
        }
    }

    impl ports::SpareDeviceSerialPort for Boundary {
        fn resolve_spare_device_serial(&mut self, requested_spare_serial: Option<u32>) -> Result<u32> {
            requested_spare_serial
                .or(self.spare_serial)
                .ok_or_else(|| anyhow::anyhow!("pass --serial in non-interactive use"))
        }
    }

    impl ports::DevicePinPolicyPort for Boundary {
        fn device_requires_pin(&mut self, _serial: u32) -> Result<bool> {
            Ok(true)
        }
    }

    impl ports::PinInputPort for Boundary {
        fn read_pin(&self) -> Result<SecretMaterial> {
            if let Some(pin_error) = self.pin_error {
                return Err(anyhow::anyhow!(pin_error));
            }
            Ok(secret_material(b"123456"))
        }
    }

    impl ports::SecretInputPort for Boundary {
        fn read_bw_email_secret(&self) -> Result<SecretMaterial> {
            Ok(secret_material(b"user@example.com"))
        }

        fn read_bw_password_secret(&self) -> Result<SecretMaterial> {
            Ok(secret_material(b"password"))
        }

        fn read_bws_access_token_secret(&self) -> Result<SecretMaterial> {
            Ok(secret_material(b"token"))
        }

        fn read_streamed_secret(&self) -> Result<SecretMaterial> {
            Err(anyhow::anyhow!("--stdin requires pipe or redirect input"))
        }
    }

    impl ports::BootstrapSecretDocumentInputPort for Boundary {
        fn read_bootstrap_secret_fields(&self) -> Result<BTreeMap<String, SecretMaterial>> {
            if let Some(stdin_json_error) = self.stdin_json_error {
                return Err(anyhow::anyhow!(stdin_json_error));
            }
            let mut fields = BTreeMap::new();
            fields.insert("bw-email".to_string(), secret_material(b"user@example.com"));
            fields.insert("bw-password".to_string(), secret_material(b"password"));
            fields.insert("bws-access-token".to_string(), secret_material(b"token"));
            Ok(fields)
        }
    }

    impl ports::SecretOutputPort for Boundary {
        fn write_secret(&self, _secret: &SecretMaterial) -> Result<()> {
            Ok(())
        }
    }

    impl ports::ReportPort for Boundary {
        fn write_enroll_report(&self, _summary: &crate::secrets::domain::values::EnrollSummary) -> Result<()> {
            Ok(())
        }

        fn write_verify_report(&self, _summary: &crate::secrets::domain::values::VerifySummary) -> Result<()> {
            Ok(())
        }
    }

    impl ports::SecretStoragePort for Boundary {
        fn inspect_secret_storage_setup(
            &mut self,
            _serial: u32,
            _probe: &SecretStorageSetupProbe,
        ) -> Result<SecretStorageSetupInspection> {
            Ok(SecretStorageSetupInspection {
                key_exists: false,
                piv_version: PivApplicationVersion::minimum_for_secret_storage(),
                pin_retries: 1,
                manifest_bytes: None,
                occupied_object_ids: vec![],
            })
        }

        fn initialize_secret_storage(
            &mut self,
            _serial: u32,
            _intent: crate::secrets::domain::storage::SecretStorageSetupIntent,
        ) -> Result<()> {
            Ok(())
        }

        fn inspect_secret_storage_write(
            &mut self,
            _serial: u32,
            _storage: &SecretStorageSpec,
        ) -> Result<SecretStorageWriteInspection> {
            Ok(SecretStorageWriteInspection {
                manifest_bytes: Some(SecretManifest::expected().encode()?),
                object_exists: false,
            })
        }

        fn store_secret(
            &mut self,
            _serial: u32,
            _intent: SecretStorageWriteIntent,
            _secret: &SecretMaterial,
        ) -> Result<()> {
            Ok(())
        }

        fn inspect_secret_storage_read(
            &mut self,
            _serial: u32,
            _storage: &SecretStorageSpec,
        ) -> Result<SecretStorageReadInspection> {
            Ok(SecretStorageReadInspection {
                manifest_bytes: Some(SecretManifest::expected().encode()?),
                encoded: Some(vec![1u8]),
            })
        }

        fn load_secret(
            &mut self,
            _serial: u32,
            _intent: &SecretStorageReadIntent,
            _pin: Option<&SecretMaterial>,
        ) -> Result<SecretMaterial> {
            Ok(secret_with_len(1))
        }
    }

    fn secret_material(bytes: &'static [u8]) -> SecretMaterial {
        SecretMaterial::from_backend(
            bytes.to_vec(),
            |secret| secret.len(),
            |secret| Ok(secret.clone()),
        )
    }

    fn secret_with_len(len: usize) -> SecretMaterial {
        SecretMaterial::from_backend(
            vec![0u8; len],
            |secret| secret.len(),
            |secret| Ok(secret.clone()),
        )
    }

    #[test]
    fn enroll_spare_rejects_same_primary_and_spare_serial() -> Result<()> {
        let mut boundary = Boundary::new(Some(10), Some(10));
        let command = EnrollSpareCommand {
            primary_serial: Some(10),
            spare_serial: Some(10),
        };
        let err = super::run_enroll_spare_with_prompt::run_enroll_spare_with_prompt(command, &mut boundary)
            .expect_err("enroll-spare accepted duplicate serials");
        assert_eq!(err.to_string(), "primary and spare YubiKey serial must be different");
        Ok(())
    }

    #[test]
    fn put_rejects_noninteractive_without_serial_before_device_open() -> Result<()> {
        let mut boundary = Boundary::new(None, None);
        let command = PutCommand { name: SecretName::BwsAccessToken, serial: None, force: false };
        let err = super::run_put_with_stdin::run_put_with_stdin(command, &mut boundary)
            .expect_err("put unexpectedly succeeded");
        assert_eq!(err.to_string(), "pass --serial in non-interactive use");
        Ok(())
    }

    #[test]
    fn setup_rejects_noninteractive_without_serial_before_device_open() -> Result<()> {
        let mut boundary = Boundary::new(None, None);
        let err = super::run_setup_with::run_setup_with(SetupCommand { serial: None }, &mut boundary)
            .expect_err("setup unexpectedly succeeded");
        assert_eq!(err.to_string(), "pass --serial in non-interactive use");
        Ok(())
    }

    #[test]
    fn put_rejects_tty_stdin_before_device_open() -> Result<()> {
        let mut boundary = Boundary::new(Some(10), None);
        let command = PutCommand { name: SecretName::BwsAccessToken, serial: Some(10), force: false };
        let err = super::run_put_with_stdin::run_put_with_stdin(command, &mut boundary)
            .expect_err("put unexpectedly accepted tty stdin");
        assert_eq!(err.to_string(), "--stdin requires pipe or redirect input");
        Ok(())
    }

    #[test]
    fn rotate_bws_token_rejects_noninteractive_without_serial() -> Result<()> {
        let mut boundary = Boundary::new(None, None);
        let err = super::run_rotate_bws_token_with_prompt::run_rotate_bws_token_with_prompt(
            RotateBwsTokenCommand { serial: None },
            &mut boundary,
        )
        .expect_err("rotate-bws-token unexpectedly succeeded");
        assert_eq!(err.to_string(), "pass --serial in non-interactive use");
        Ok(())
    }

    #[test]
    fn enroll_primary_rejects_tty_stdin_json_before_device_open() -> Result<()> {
        let mut boundary = Boundary::new(Some(10), None)
            .with_stdin_json_error("--stdin-json requires pipe or redirect input");
        let err = super::run_enroll_primary_with_stdin_json::run_enroll_primary_with_stdin_json(
            EnrollPrimaryCommand { serial: Some(10) },
            &mut boundary,
        )
        .expect_err("enroll-primary unexpectedly accepted tty stdin-json");
        assert_eq!(err.to_string(), "--stdin-json requires pipe or redirect input");
        Ok(())
    }

    #[test]
    fn enroll_spare_rejects_tty_stdin_json_before_device_open() -> Result<()> {
        let mut boundary = Boundary::new(Some(10), Some(20))
            .with_stdin_json_error("--stdin-json requires pipe or redirect input");
        let command = EnrollSpareCommand { primary_serial: Some(10), spare_serial: Some(20) };
        let err = super::run_enroll_spare_with_stdin_json::run_enroll_spare_with_stdin_json(command, &mut boundary)
            .expect_err("enroll-spare unexpectedly accepted tty stdin-json");
        assert_eq!(err.to_string(), "--stdin-json requires pipe or redirect input");
        Ok(())
    }

    #[test]
    fn enroll_primary_stdin_json_stops_before_secret_read_when_pin_verification_fails() -> Result<()> {
        let mut boundary = Boundary::new(Some(10), None).with_pin_error("pin verification failed");
        let err = super::run_enroll_primary_with_stdin_json::run_enroll_primary_with_stdin_json(
            EnrollPrimaryCommand { serial: Some(10) },
            &mut boundary,
        )
        .expect_err("enroll-primary unexpectedly succeeded");
        assert_eq!(err.to_string(), "pin verification failed");
        Ok(())
    }

    #[test]
    fn enroll_spare_stdin_json_stops_before_secret_read_when_pin_verification_fails() -> Result<()> {
        let mut boundary = Boundary::new(Some(10), Some(20)).with_pin_error("pin verification failed");
        let command = EnrollSpareCommand { primary_serial: Some(10), spare_serial: Some(20) };
        let err = super::run_enroll_spare_with_stdin_json::run_enroll_spare_with_stdin_json(command, &mut boundary)
            .expect_err("enroll-spare unexpectedly succeeded");
        assert_eq!(err.to_string(), "pin verification failed");
        Ok(())
    }
}
