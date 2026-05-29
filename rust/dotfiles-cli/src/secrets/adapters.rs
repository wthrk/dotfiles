//! secrets adapter 層の公開境界。
//!
//! この module は port trait を実装する adapter 型だけを公開し、外部 API・端末・
//! YubiKey storage を port 契約へ翻訳する境界に限定する。

mod bws_client;
mod piv_io;

use crate::secrets::ports::{
    BootstrapSecretDocumentInputPort, BwsClientPort, DevicePinPolicyPort, DeviceSerialPort,
    PinInputPort, ReportPort, RotationContinuationPort, SecretInputPort, SecretOutputPort,
    SecretStoragePort, SpareDeviceSerialPort,
};
use crate::{Result, secrets::support::protection::ProtectedSecret};

/// YubiKey discovery と PIN 要否判定を port 契約へ翻訳する adapter。
#[derive(Default)]
pub(crate) struct DeviceSelectionAdapter(piv_io::DeviceSelectionAdapter);

impl DeviceSerialPort for DeviceSelectionAdapter {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
        self.0.resolve_device_serial(requested)
    }
}

impl SpareDeviceSerialPort for DeviceSelectionAdapter {
    fn resolve_spare_device_serial(&mut self, requested_spare_serial: Option<u32>) -> Result<u32> {
        self.0.resolve_spare_device_serial(requested_spare_serial)
    }
}

impl DevicePinPolicyPort for DeviceSelectionAdapter {
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool> {
        self.0.device_requires_pin(serial)
    }
}

/// process I/O と secret 入出力を port 契約へ翻訳する adapter。
#[derive(Default)]
pub(crate) struct ProcessIoAdapter(piv_io::ProcessIoAdapter);

impl PinInputPort for ProcessIoAdapter {
    fn read_pin(&self) -> Result<ProtectedSecret> {
        self.0.read_pin()
    }
}

impl SecretInputPort for ProcessIoAdapter {
    fn read_bw_email_secret(&self) -> Result<ProtectedSecret> {
        self.0.read_bw_email_secret()
    }

    fn read_bw_password_secret(&self) -> Result<ProtectedSecret> {
        self.0.read_bw_password_secret()
    }

    fn read_bws_access_token_secret(&self) -> Result<ProtectedSecret> {
        self.0.read_bws_access_token_secret()
    }

    fn read_streamed_secret(&self) -> Result<ProtectedSecret> {
        self.0.read_streamed_secret()
    }
}

impl RotationContinuationPort for ProcessIoAdapter {
    fn continue_rotation(&self) -> Result<bool> {
        self.0.continue_rotation()
    }
}

impl BootstrapSecretDocumentInputPort for ProcessIoAdapter {
    fn read_bootstrap_secret_fields(
        &self,
    ) -> Result<std::collections::BTreeMap<String, ProtectedSecret>> {
        self.0.read_bootstrap_secret_fields()
    }
}

impl SecretOutputPort for ProcessIoAdapter {
    fn write_secret(&self, secret: &ProtectedSecret) -> Result<()> {
        self.0.write_secret(secret)
    }
}

/// YubiKey storage I/O を port 契約へ翻訳する adapter。
#[derive(Default)]
pub(crate) struct StorageAdapter(piv_io::StorageAdapter);

impl SecretStoragePort for StorageAdapter {
    fn inspect_secret_storage_setup(
        &mut self,
        serial: u32,
        probe: &crate::secrets::domain::storage::SecretStorageSetupProbe,
    ) -> Result<crate::secrets::domain::storage::SecretStorageSetupInspection> {
        self.0.inspect_secret_storage_setup(serial, probe)
    }

    fn initialize_secret_storage(
        &mut self,
        serial: u32,
        intent: crate::secrets::domain::storage::SecretStorageSetupIntent,
    ) -> Result<()> {
        self.0.initialize_secret_storage(serial, intent)
    }

    fn finalize_secret_storage_setup(
        &mut self,
        serial: u32,
        intent: crate::secrets::domain::storage::SecretStorageSetupIntent,
    ) -> Result<()> {
        self.0.finalize_secret_storage_setup(serial, intent)
    }

    fn inspect_secret_storage_write(
        &mut self,
        serial: u32,
        storage: &crate::secrets::domain::piv::SecretStorageSpec,
    ) -> Result<crate::secrets::domain::storage::SecretStorageWriteInspection> {
        self.0.inspect_secret_storage_write(serial, storage)
    }

    fn store_secret(
        &mut self,
        serial: u32,
        intent: crate::secrets::domain::storage::SecretStorageWriteIntent,
        secret: &ProtectedSecret,
    ) -> Result<()> {
        self.0.store_secret(serial, intent, secret)
    }

    fn inspect_secret_storage_read(
        &mut self,
        serial: u32,
        storage: &crate::secrets::domain::piv::SecretStorageSpec,
    ) -> Result<crate::secrets::domain::storage::SecretStorageReadInspection> {
        self.0.inspect_secret_storage_read(serial, storage)
    }

    fn load_secret(
        &mut self,
        serial: u32,
        intent: &crate::secrets::domain::storage::SecretStorageReadIntent,
        pin: Option<&ProtectedSecret>,
    ) -> Result<ProtectedSecret> {
        self.0.load_secret(serial, intent, pin)
    }
}

/// CLI JSON report 出力を port 契約へ翻訳する adapter。
#[derive(Default)]
pub(crate) struct JsonReportAdapter(piv_io::JsonReportAdapter);

impl ReportPort for JsonReportAdapter {
    fn write_enroll_report(
        &self,
        summary: &crate::secrets::domain::values::EnrollSummary,
    ) -> Result<()> {
        self.0.write_enroll_report(summary)
    }

    fn write_verify_report(
        &self,
        summary: &crate::secrets::domain::values::VerifySummary,
    ) -> Result<()> {
        self.0.write_verify_report(summary)
    }
}

/// Bitwarden Secrets Manager 取得を port 契約へ翻訳する adapter。
#[derive(Default)]
pub(crate) struct BwsClientAdapter(bws_client::BwsClientAdapter);

impl BwsClientPort for BwsClientAdapter {
    async fn list_bws_projects(
        &self,
        access_token: &ProtectedSecret,
    ) -> Result<
        Vec<
            crate::secrets::domain::values::BwsLookupCandidate<
                crate::secrets::domain::values::BwsProjectId,
            >,
        >,
    > {
        self.0.list_bws_projects(access_token).await
    }

    async fn list_bws_secrets(
        &self,
        access_token: &ProtectedSecret,
        project_id: &crate::secrets::domain::values::BwsProjectId,
    ) -> Result<
        Vec<
            crate::secrets::domain::values::BwsLookupCandidate<
                crate::secrets::domain::values::BwsSecretId,
            >,
        >,
    > {
        self.0.list_bws_secrets(access_token, project_id).await
    }

    async fn fetch_bws_secret_by_id(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &crate::secrets::domain::values::BwsSecretId,
    ) -> Result<ProtectedSecret> {
        self.0.fetch_bws_secret_by_id(access_token, secret_id).await
    }
}
