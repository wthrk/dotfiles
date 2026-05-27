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
            storage::{
                SecretStorageReadInspection, SecretStorageReadIntent, SecretStorageSetupInspection,
                SecretStorageSetupIntent, SecretStorageSetupProbe, SecretStorageWriteInspection,
                SecretStorageWriteIntent,
            },
            values::{EnrollSummary, VerifySummary},
        },
        ports::{
            BootstrapSecretDocumentInputPort, DevicePinPolicyPort, DeviceSerialPort, PinInputPort,
            ReportPort, SecretInputPort, SecretOutputPort, SecretStoragePort,
            SpareDeviceSerialPort,
        },
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeviceCandidate {
    serial: u32,
    label: String,
}

trait RealDeviceIo {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>>;
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
    device: piv_io::DeviceSelectionAdapter,
    process_io: piv_io::ProcessIoAdapter,
    storage: piv_io::StorageAdapter,
    report: piv_io::JsonReportAdapter,
}

impl DeviceSerialPort for SecretsAdapters {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
        self.device.resolve_device_serial(requested)
    }
}

impl SpareDeviceSerialPort for SecretsAdapters {
    fn resolve_spare_device_serial(&mut self, requested_spare_serial: Option<u32>) -> Result<u32> {
        self.device
            .resolve_spare_device_serial(requested_spare_serial)
    }
}

impl DevicePinPolicyPort for SecretsAdapters {
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool> {
        self.device.device_requires_pin(serial)
    }
}

impl PinInputPort for SecretsAdapters {
    fn read_pin(&self) -> Result<SecretMaterial> {
        self.process_io.read_pin()
    }
}

impl SecretInputPort for SecretsAdapters {
    fn read_named_secret(&self, name: SecretName) -> Result<SecretMaterial> {
        self.process_io.read_named_secret(name)
    }

    fn read_streamed_secret(&self) -> Result<SecretMaterial> {
        self.process_io.read_streamed_secret()
    }
}

impl BootstrapSecretDocumentInputPort for SecretsAdapters {
    fn read_bootstrap_secret_document(&self) -> Result<BootstrapSecretDocument> {
        self.process_io.read_bootstrap_secret_document()
    }
}

impl SecretOutputPort for SecretsAdapters {
    fn write_secret(&self, secret: &SecretMaterial) -> Result<()> {
        self.process_io.write_secret(secret)
    }
}

impl SecretStoragePort for SecretsAdapters {
    fn inspect_secret_storage_setup(
        &mut self,
        serial: u32,
        probe: &SecretStorageSetupProbe,
    ) -> Result<SecretStorageSetupInspection> {
        self.storage.inspect_secret_storage_setup(serial, probe)
    }

    fn initialize_secret_storage(
        &mut self,
        serial: u32,
        intent: SecretStorageSetupIntent,
    ) -> Result<()> {
        self.storage.initialize_secret_storage(serial, intent)
    }

    fn inspect_secret_storage_write(
        &mut self,
        serial: u32,
        storage: &crate::secrets::domain::piv::SecretStorageSpec,
    ) -> Result<SecretStorageWriteInspection> {
        self.storage.inspect_secret_storage_write(serial, storage)
    }

    fn store_secret(
        &mut self,
        serial: u32,
        intent: SecretStorageWriteIntent,
        secret: &SecretMaterial,
    ) -> Result<()> {
        self.storage.store_secret(serial, intent, secret)
    }

    fn inspect_secret_storage_read(
        &mut self,
        serial: u32,
        storage: &crate::secrets::domain::piv::SecretStorageSpec,
    ) -> Result<SecretStorageReadInspection> {
        self.storage.inspect_secret_storage_read(serial, storage)
    }

    fn load_secret(
        &mut self,
        serial: u32,
        intent: SecretStorageReadIntent,
        pin: Option<&SecretMaterial>,
    ) -> Result<SecretMaterial> {
        self.storage.load_secret(serial, intent, pin)
    }
}

impl ReportPort for SecretsAdapters {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()> {
        self.report.write_enroll_report(summary)
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        self.report.write_verify_report(summary)
    }
}
