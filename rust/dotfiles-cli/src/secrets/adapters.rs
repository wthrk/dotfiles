//! secrets adapter 層の公開境界。
//!
//! adapter 下位 module をそのまま露出せず、entrypoint が使う runtime adapter 生成だけを提供する。

mod gpg_io;
mod piv_io;

use std::collections::BTreeMap;

use crate::{
    Result,
    secrets::{
        domain::{
            material::SecretMaterial,
            storage::{
                SecretStorageReadInspection, SecretStorageReadIntent, SecretStorageSetupInspection,
                SecretStorageSetupIntent, SecretStorageSetupProbe, SecretStorageWriteInspection,
                SecretStorageWriteIntent,
            },
            values::{EnrollSummary, VerifySummary},
        },
        ports::{
            BootstrapSecretDocumentInputPort, DevicePinPolicyPort, DeviceSerialPort,
            GpgRecoveryPort, PinInputPort, ReportPort, RotationContinuationPort, SecretInputPort,
            SecretOutputPort, SecretStoragePort, SpareDeviceSerialPort, SshPublicKeyOutputPort,
        },
    },
};

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
    gpg: gpg_io::GpgRecoveryAdapter,
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
    fn read_bw_email_secret(&self) -> Result<SecretMaterial> {
        self.process_io.read_bw_email_secret()
    }

    fn read_bw_password_secret(&self) -> Result<SecretMaterial> {
        self.process_io.read_bw_password_secret()
    }

    fn read_bws_access_token_secret(&self) -> Result<SecretMaterial> {
        self.process_io.read_bws_access_token_secret()
    }

    fn read_streamed_secret(&self) -> Result<SecretMaterial> {
        self.process_io.read_streamed_secret()
    }
}

impl RotationContinuationPort for SecretsAdapters {
    fn continue_rotation(&self) -> Result<bool> {
        self.process_io.continue_rotation()
    }
}

impl BootstrapSecretDocumentInputPort for SecretsAdapters {
    fn read_bootstrap_secret_fields(&self) -> Result<BTreeMap<String, SecretMaterial>> {
        self.process_io.read_bootstrap_secret_fields()
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

    fn finalize_secret_storage_setup(
        &mut self,
        serial: u32,
        intent: SecretStorageSetupIntent,
    ) -> Result<()> {
        self.storage.finalize_secret_storage_setup(serial, intent)
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
        intent: &SecretStorageReadIntent,
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

impl GpgRecoveryPort for SecretsAdapters {
    fn read_gpg_secret_key_backup(&self, bws_access_token: &SecretMaterial) -> Result<String> {
        self.gpg.read_gpg_secret_key_backup(bws_access_token)
    }

    fn import_gpg_secret_key(&self, armored_secret_key: &str) -> Result<()> {
        self.gpg.import_gpg_secret_key(armored_secret_key)
    }

    fn verify_gpg_restore_prerequisites(&self) -> Result<()> {
        self.gpg.verify_gpg_restore_prerequisites()
    }

    fn export_ssh_public_key(&self) -> Result<String> {
        self.gpg.export_ssh_public_key()
    }
}

impl SshPublicKeyOutputPort for SecretsAdapters {
    fn write_ssh_public_key(&self, public_key: &str) -> Result<()> {
        self.gpg.write_ssh_public_key(public_key)
    }
}
