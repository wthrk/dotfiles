//! secrets adapter 層の公開境界。
//!
//! adapter 下位 module をそのまま露出せず、entrypoint が使う runtime adapter 生成だけを提供する。

mod piv_io;
mod yubikey;

use crate::{
    Result,
    secrets::{
        domain::{
            manifest::BootstrapSecretDocument,
            material::SecretMaterial,
            piv::{SecretName, SecretStorageSpec},
            values::{EnrollSummary, VerifySummary},
        },
        ports::{
            BootstrapSecretDocumentInputPort, DevicePinPolicyPort, DeviceSerialPort, PinInputPort,
            ReportPort, SecretInputPort, SecretOutputPort, SecretStoragePort,
            SpareDeviceSerialPort,
        },
    },
};

trait RealDeviceIo {
    fn discover_devices(&mut self) -> Result<Vec<crate::secrets::ports::DeviceCandidate>>;
    fn open_device_by_serial(&mut self, serial: u32) -> Result<yubikey::YubikeySecretDevice>;
}

trait SecretDeviceIo {
    fn key_exists(&mut self) -> Result<bool>;
    fn check_key_generation_preconditions(&mut self) -> Result<()>;
    fn check_management_auth_preconditions(&mut self) -> Result<()>;
    fn generate_key(&mut self) -> Result<()>;
    fn read_object(
        &mut self,
        object_id: crate::secrets::domain::piv::PivObjectId,
    ) -> Result<Option<Vec<u8>>>;
    fn write_object(
        &mut self,
        object_id: crate::secrets::domain::piv::PivObjectId,
        value: &mut [u8],
    ) -> Result<()>;
    fn requires_pin_input(&self) -> bool;
    fn verify_pin(&mut self, pin: &SecretMaterial) -> Result<()>;
    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        plaintext: &SecretMaterial,
    ) -> Result<Vec<u8>>;
    fn open_from_storage(
        &mut self,
        storage: SecretStorageSpec,
        encoded: &[u8],
    ) -> Result<SecretMaterial>;
}

/// CLI entrypoint が利用する secrets runtime adapter。
///
/// 公開面は port trait 実装型としてのこの型に限定し、下位 adapter module や
/// factory/helper 関数を crate 公開しない。
#[derive(Default)]
pub(crate) struct SecretsAdapters {
    boundary: piv_io::RealSecretsBoundary,
}

impl DeviceSerialPort for SecretsAdapters {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
        self.boundary.resolve_device_serial(requested)
    }
}

impl SpareDeviceSerialPort for SecretsAdapters {
    fn resolve_spare_device_serial(&mut self, requested_spare_serial: Option<u32>) -> Result<u32> {
        self.boundary
            .resolve_spare_device_serial(requested_spare_serial)
    }
}

impl DevicePinPolicyPort for SecretsAdapters {
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool> {
        self.boundary.device_requires_pin(serial)
    }
}

impl PinInputPort for SecretsAdapters {
    fn read_pin(&self) -> Result<SecretMaterial> {
        self.boundary.read_pin()
    }
}

impl SecretInputPort for SecretsAdapters {
    fn read_visible_secret(&self) -> Result<SecretMaterial> {
        self.boundary.read_visible_secret()
    }

    fn read_hidden_secret(&self, name: SecretName) -> Result<SecretMaterial> {
        self.boundary.read_hidden_secret(name)
    }

    fn read_stdin_secret(&self) -> Result<SecretMaterial> {
        self.boundary.read_stdin_secret()
    }
}

impl BootstrapSecretDocumentInputPort for SecretsAdapters {
    fn read_bootstrap_secret_document_noninteractive(&self) -> Result<BootstrapSecretDocument> {
        self.boundary
            .read_bootstrap_secret_document_noninteractive()
    }
}

impl SecretOutputPort for SecretsAdapters {
    fn write_secret(&self, secret: &SecretMaterial) -> Result<()> {
        self.boundary.write_secret(secret)
    }
}

impl SecretStoragePort for SecretsAdapters {
    fn initialize_secret_storage(&mut self, serial: u32) -> Result<()> {
        self.boundary.initialize_secret_storage(serial)
    }

    fn store_secret(
        &mut self,
        serial: u32,
        storage: SecretStorageSpec,
        secret: &SecretMaterial,
    ) -> Result<()> {
        self.boundary.store_secret(serial, storage, secret)
    }

    fn put_secret(
        &mut self,
        serial: u32,
        storage: SecretStorageSpec,
        secret: &SecretMaterial,
        force: bool,
    ) -> Result<()> {
        self.boundary.put_secret(serial, storage, secret, force)
    }

    fn load_secret(
        &mut self,
        serial: u32,
        storage: SecretStorageSpec,
        pin: Option<&SecretMaterial>,
    ) -> Result<SecretMaterial> {
        self.boundary.load_secret(serial, storage, pin)
    }

    fn verify_local_storage(&mut self, serial: u32, pin: Option<&SecretMaterial>) -> Result<()> {
        self.boundary.verify_local_storage(serial, pin)
    }
}

impl ReportPort for SecretsAdapters {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()> {
        self.boundary.write_enroll_report(summary)
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        self.boundary.write_verify_report(summary)
    }
}
