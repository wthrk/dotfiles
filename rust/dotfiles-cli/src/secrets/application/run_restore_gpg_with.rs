//! restore-gpg の順序責務を保持し、BWS/GPG 実体依存を port 境界へ固定する。

use crate::Result;
use crate::secrets::{
    domain::{
        piv::{SecretName, validate_piv_pin_len},
        storage::SecretStorageReadIntent,
        values::RestoreGpgCommand,
    },
    ports::{self, SecretStoragePort},
};

/// BWS backup から GPG secret key を復元し、必須 subkey / agent 前提を検証する。
///
/// token 取得は既存 YubiKey storage 経路を再利用し、BWS/GPG 実体依存は `GpgRecoveryPort`
/// の呼び出しへ限定することで、application 層に外部 SDK / process I/O を持ち込まない。
pub(crate) fn run_restore_gpg_with<
    B: ports::DeviceSerialPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + SecretStoragePort
        + ports::GpgRecoveryPort,
>(
    command: RestoreGpgCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = boundary.resolve_device_serial(command.serial)?;
    let pin = if boundary.device_requires_pin(serial)? {
        let pin = boundary.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };
    let storage = SecretName::BwsAccessToken.storage_spec(serial);
    let inspection = boundary.inspect_secret_storage_read(serial, &storage)?;
    let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
    let token = boundary
        .load_secret(serial, &intent, pin.as_ref())
        .map_err(|error| intent.decode_error(error))?;
    intent.validate_loaded_secret(&token)?;

    let armored_secret_key = boundary.read_gpg_secret_key_backup(&token)?;
    boundary.import_gpg_secret_key(&armored_secret_key)?;
    boundary.verify_gpg_restore_prerequisites()
}

#[cfg(test)]
mod tests {
    use crate::Result;
    use crate::secrets::{
        domain::{
            material::SecretMaterial,
            piv::SecretStorageSpec,
            storage::{SecretStorageReadInspection, SecretStorageReadIntent},
            values::RestoreGpgCommand,
        },
        ports::{self, SecretStoragePort},
    };

    use super::run_restore_gpg_with;

    fn material_from_bytes(bytes: &[u8]) -> SecretMaterial {
        SecretMaterial::from_backend(
            bytes.to_vec(),
            |value| value.len(),
            |value| Ok(value.clone()),
        )
    }

    struct Boundary;

    impl ports::DeviceSerialPort for Boundary {
        fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
            Ok(requested.unwrap_or(2001))
        }
    }

    impl ports::DevicePinPolicyPort for Boundary {
        fn device_requires_pin(&mut self, _serial: u32) -> Result<bool> {
            Ok(false)
        }
    }

    impl ports::PinInputPort for Boundary {
        fn read_pin(&self) -> Result<SecretMaterial> {
            Ok(material_from_bytes(b"123456"))
        }
    }

    impl SecretStoragePort for Boundary {
        fn inspect_secret_storage_setup(
            &mut self,
            _serial: u32,
            _probe: &crate::secrets::domain::storage::SecretStorageSetupProbe,
        ) -> Result<crate::secrets::domain::storage::SecretStorageSetupInspection> {
            unreachable!()
        }

        fn initialize_secret_storage(
            &mut self,
            _serial: u32,
            _intent: crate::secrets::domain::storage::SecretStorageSetupIntent,
        ) -> Result<()> {
            unreachable!()
        }

        fn finalize_secret_storage_setup(
            &mut self,
            _serial: u32,
            _intent: crate::secrets::domain::storage::SecretStorageSetupIntent,
        ) -> Result<()> {
            unreachable!()
        }

        fn inspect_secret_storage_write(
            &mut self,
            _serial: u32,
            _storage: &SecretStorageSpec,
        ) -> Result<crate::secrets::domain::storage::SecretStorageWriteInspection> {
            unreachable!()
        }

        fn store_secret(
            &mut self,
            _serial: u32,
            _intent: crate::secrets::domain::storage::SecretStorageWriteIntent,
            _secret: &SecretMaterial,
        ) -> Result<()> {
            unreachable!()
        }

        fn inspect_secret_storage_read(
            &mut self,
            _serial: u32,
            _storage: &SecretStorageSpec,
        ) -> Result<SecretStorageReadInspection> {
            Ok(SecretStorageReadInspection {
                manifest_bytes: Some(
                    crate::secrets::domain::manifest::SecretManifest::expected().encode()?,
                ),
                encoded: Some(vec![1]),
            })
        }

        fn load_secret(
            &mut self,
            _serial: u32,
            _intent: &SecretStorageReadIntent,
            _pin: Option<&SecretMaterial>,
        ) -> Result<SecretMaterial> {
            Ok(material_from_bytes(b"bws-token"))
        }
    }

    impl ports::GpgRecoveryPort for Boundary {
        fn read_gpg_secret_key_backup(&self, _bws_access_token: &SecretMaterial) -> Result<String> {
            Ok(
                "-----BEGIN PGP PRIVATE KEY BLOCK-----\n...\n-----END PGP PRIVATE KEY BLOCK-----"
                    .to_string(),
            )
        }

        fn import_gpg_secret_key(&self, _armored_secret_key: &str) -> Result<()> {
            Ok(())
        }

        fn verify_gpg_restore_prerequisites(&self) -> Result<()> {
            Ok(())
        }

        fn export_ssh_public_key(&self) -> Result<String> {
            unreachable!()
        }
    }

    #[test]
    fn restore_gpg_reads_token_and_runs_recovery_steps() -> Result<()> {
        let mut boundary = Boundary;
        run_restore_gpg_with(RestoreGpgCommand { serial: Some(2001) }, &mut boundary)
    }
}
