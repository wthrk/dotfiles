use anyhow::bail;

use crate::Result;
use crate::secrets::{
    domain::{
        blob::SecretBlob,
        manifest::SecretManifest,
        piv::{PivObjectId, SecretName},
        values::{VerifySummary, VerifyYubikeyCommand},
    },
    ports::{self, SecretDevice},
};

/// 保存済み secret の存在と、要求された外部確認項目を検証する。
///
/// local storage 検証を完了条件の先頭に固定し、未実装の外部確認は report 境界で通知して
/// 明示的に停止することで、verify 結果の責任範囲を曖昧にしない。
pub(crate) fn run_verify_yubikey_with<
    B: ports::DevicePinPolicyPort
        + ports::PinInputPort
        + ports::DeviceSelectionPort
        + ports::ReportPort,
>(
    command: VerifyYubikeyCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = command.required_serial()?;
    let requested = command.requested_external_checks()?;
    let pin = if boundary.device_requires_pin(serial)? {
        Some(boundary.read_pin()?)
    } else {
        None
    };
    let mut device = boundary.open_device_by_serial(serial)?;
    if device.requires_pin_input() {
        let Some(pin) = pin.as_ref() else {
            bail!("PIN is required for this operation");
        };
        device.verify_pin(pin)?;
    }
    SecretManifest::decode_initialized(device.read_object(PivObjectId::MANIFEST)?.as_deref())?;
    for name in SecretName::iter() {
        let encoded = device
            .read_object(name.object_id())?
            .ok_or_else(|| anyhow::anyhow!("{name} is not stored on this YubiKey"))?;
        let blob = SecretBlob::decode_for_name(&encoded, name)?;
        let content_key = device.unwrap_key(&blob.wrapped_key)?;
        let secret = blob.decrypt_secret(device.serial(), &content_key)?;
        secret.with_bytes(|bytes| name.ensure_value_non_empty(bytes))?;
    }
    if !requested.is_empty() {
        boundary.write_verify_report(&VerifySummary::external_checks_unavailable(
            serial,
            requested.iter().copied(),
        ))?;
        let requested_names = requested
            .iter()
            .map(|check| check.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        bail!("external checks are not implemented yet: {requested_names}");
    }

    boundary.write_verify_report(&VerifySummary::local_storage_verified(serial))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::domain::{material::SecretMaterial, values::ExternalCheck};

    #[derive(Default)]
    struct FakeBoundary {
        requires_pin: bool,
        verify_calls: usize,
    }

    struct FakeDevice;

    impl ports::DevicePinPolicyPort for FakeBoundary {
        fn device_requires_pin(&mut self, _serial: u32) -> Result<bool> {
            Ok(self.requires_pin)
        }
    }
    impl ports::PinInputPort for FakeBoundary {
        fn read_pin(&self) -> Result<SecretMaterial> {
            SecretMaterial::copy_from_slice(b"123456")
        }
    }
    impl ports::DeviceSelectionPort for FakeBoundary {
        type Device = FakeDevice;
        fn discover_devices(&mut self) -> Result<Vec<ports::DeviceCandidate>> {
            Ok(vec![ports::DeviceCandidate {
                serial: 2001,
                label: "fake".to_string(),
            }])
        }
        fn open_device_by_serial(&mut self, _serial: u32) -> Result<Self::Device> {
            self.verify_calls += 1;
            Ok(FakeDevice)
        }
    }
    impl ports::ReportPort for FakeBoundary {
        fn write_enroll_report(
            &self,
            _summary: &crate::secrets::domain::values::EnrollSummary,
        ) -> Result<()> {
            unreachable!()
        }
        fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
            let _ = summary;
            Ok(())
        }
    }

    impl SecretDevice for FakeDevice {
        fn serial(&self) -> u32 {
            2001
        }
        fn key_exists(&mut self) -> Result<bool> {
            Ok(true)
        }
        fn check_key_generation_preconditions(&mut self) -> Result<()> {
            Ok(())
        }
        fn check_management_auth_preconditions(&mut self) -> Result<()> {
            Ok(())
        }
        fn generate_key(&mut self) -> Result<()> {
            Ok(())
        }
        fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>> {
            if object_id == PivObjectId::MANIFEST {
                return Ok(Some(
                    crate::secrets::domain::manifest::SecretManifest::expected().encode()?,
                ));
            }
            let name = SecretName::iter()
                .find(|candidate| candidate.object_id() == object_id)
                .ok_or_else(|| anyhow::anyhow!("unknown object id"))?;
            let nonce = [0u8; 12];
            let key = SecretMaterial::copy_from_slice(&[0u8; 32])?;
            let cipher = key.with_bytes(crate::secrets::support::aead::aes_256_gcm_from_key)?;
            let mut ciphertext = SecretMaterial::copy_from_slice(b"value")?;
            let tag = ciphertext.with_secret_mut(|bytes| {
                crate::secrets::support::aead::encrypt_detached(
                    &cipher,
                    &nonce,
                    &name.additional_data(2001),
                    bytes,
                )
            })?;
            let blob = crate::secrets::domain::blob::SecretBlob {
                name,
                nonce,
                wrapped_key: vec![1u8; 32],
                ciphertext,
                tag,
            };
            Ok(Some(blob.encode()?))
        }
        fn write_object(&mut self, _object_id: PivObjectId, _value: &mut [u8]) -> Result<()> {
            Ok(())
        }
        fn wrap_key(&mut self, _key: &SecretMaterial) -> Result<Vec<u8>> {
            Ok(vec![])
        }
        fn requires_pin_input(&self) -> bool {
            false
        }
        fn verify_pin(&mut self, _pin: &SecretMaterial) -> Result<()> {
            Ok(())
        }
        fn unwrap_key(&mut self, _wrapped_key: &[u8]) -> Result<SecretMaterial> {
            SecretMaterial::copy_from_slice(&[0u8; 32])
        }
    }

    #[test]
    fn verify_requests_pin_when_required() {
        let mut boundary = FakeBoundary {
            requires_pin: true,
            ..Default::default()
        };
        let result = run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: Some(2001),
                checks: vec![],
                all: false,
            },
            &mut boundary,
        );
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(boundary.verify_calls, 1);
    }

    #[test]
    fn verify_stops_when_external_checks_requested() {
        let mut boundary = FakeBoundary::default();
        let result = run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: Some(2001),
                checks: vec![ExternalCheck::Bws],
                all: false,
            },
            &mut boundary,
        );
        assert!(result.is_err());
    }
}
