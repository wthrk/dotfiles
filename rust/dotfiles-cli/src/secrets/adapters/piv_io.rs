//! YubiKey PIV discovery/selection と実プロセス I/O を port 契約へ接続する adapter。

mod console_io;
mod device;
#[cfg(feature = "secrets-test-stub")]
mod device_test_stub;
mod report;
mod secret_io;

use anyhow::bail;

use crate::{
    Result,
    secrets::domain::{
        BootstrapSecretDocument, CheckName, EnrollSummary, SecretName, VerifySummary,
    },
    secrets::ports::{
        BootstrapSecretLoadPort, BootstrapSecretStorePort, DeviceSelectionInputPort,
        DeviceSelectionPort, DeviceSerialPort, PinInputPort, RandomBytesPort, ReportPort,
        SecretDevice, SecretInputPort, SecretLoadPort, SecretOutputPort, SecretStorePort,
        SpareDeviceSerialPort, SpareDeviceWaitPort, StorageSetupPort, StorageVerifyPort,
    },
};

use self::{
    device::{DiscoveredDevice, SecretDeviceExt, SelectedDeviceAdapter},
    report::JsonReportAdapter,
    secret_io::RealSecretIoAdapter,
};

pub(crate) type SelectedSecretsBoundary = RealSecretsBoundary<SelectedDeviceAdapter>;

pub(crate) fn build_selected_secrets_boundary() -> SelectedSecretsBoundary {
    RealSecretsBoundary::production()
}

/// 実機 device・実プロセス I/O・report 出力を束ねる runtime adapter。
pub(crate) struct RealSecretsBoundary<D = SelectedDeviceAdapter>
where
    D: DeviceSelectionPort<DeviceCandidate = DiscoveredDevice>,
{
    device: D,
    secret_io: RealSecretIoAdapter,
    report: JsonReportAdapter,
}

impl Default for RealSecretsBoundary<SelectedDeviceAdapter> {
    fn default() -> Self {
        Self::production()
    }
}

impl RealSecretsBoundary<SelectedDeviceAdapter> {
    /// production ルートで使う実機 YubiKey adapter を束ねて境界を構築する。
    pub(crate) fn production() -> Self {
        Self {
            device: SelectedDeviceAdapter::production(),
            secret_io: RealSecretIoAdapter,
            report: JsonReportAdapter,
        }
    }
}

impl<D> RealSecretsBoundary<D>
where
    D: DeviceSelectionPort<DeviceCandidate = DiscoveredDevice>,
    D::Device: SecretDeviceExt,
{
    fn choose_device(&self, devices: &[DiscoveredDevice]) -> Result<u32> {
        console_io::choose_device_serial(devices)
    }

    fn with_device<T>(
        &mut self,
        serial: u32,
        operation: impl FnOnce(&mut D::Device, &mut Self) -> Result<T>,
    ) -> Result<T> {
        let mut device = self.device.open_device_by_serial(serial)?;
        operation(&mut device, self)
    }

    /// 読み出し系処理の前に PIN 検証を強制し、秘密値復号を許可する。
    fn ensure_pin_verified(&self, device: &mut D::Device) -> Result<()> {
        if device.requires_pin_input() {
            let pin = self.read_pin()?;
            device.verify_pin(pin.as_ref())?;
        }
        Ok(())
    }

    fn with_verified_device<T>(
        &mut self,
        serial: u32,
        operation: impl FnOnce(&mut D::Device, &mut Self) -> Result<T>,
    ) -> Result<T> {
        self.with_device(serial, |device, boundary| {
            boundary.ensure_pin_verified(device)?;
            operation(device, boundary)
        })
    }
}

impl<D> DeviceSerialPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort<DeviceCandidate = DiscoveredDevice>,
    D::Device: SecretDeviceExt,
{
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
        match requested {
            Some(serial) => Ok(serial),
            None => {
                let devices = self.device.discover_devices()?;
                self.choose_device(&devices)
            }
        }
    }
}

impl<D> DeviceSelectionPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort<DeviceCandidate = DiscoveredDevice>,
    D::Device: SecretDeviceExt,
{
    type Device = D::Device;
    type DeviceCandidate = DiscoveredDevice;

    fn discover_devices(&mut self) -> Result<Vec<Self::DeviceCandidate>> {
        self.device.discover_devices()
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<Self::Device> {
        self.device.open_device_by_serial(serial)
    }
}

impl<D> DeviceSelectionInputPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort<DeviceCandidate = DiscoveredDevice>,
    D::Device: SecretDeviceExt,
{
    fn choose_device(&self, devices: &[Self::DeviceCandidate]) -> Result<u32> {
        RealSecretsBoundary::choose_device(self, devices)
    }
}

impl<D> SpareDeviceSerialPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort<DeviceCandidate = DiscoveredDevice>,
    D::Device: SecretDeviceExt,
{
    fn resolve_spare_device_serial(
        &mut self,
        primary_serial: Option<u32>,
        spare_serial: Option<u32>,
    ) -> Result<u32> {
        if let Some(serial) = spare_serial {
            if Some(serial) == primary_serial {
                bail!("primary and spare YubiKey serial must be different");
            }
            return Ok(serial);
        }

        loop {
            let devices = self.device.discover_devices()?;
            let serial = self.choose_device(&devices)?;
            if Some(serial) != primary_serial {
                return Ok(serial);
            }
            self.wait_for_spare_device()?;
        }
    }
}

impl<D> SpareDeviceWaitPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort<DeviceCandidate = DiscoveredDevice>,
{
    fn wait_for_spare_device(&self) -> Result<()> {
        let _ = console_io::read_prompt_line("Insert spare YubiKey and press Enter to continue: ")?;
        Ok(())
    }
}

impl<D> PinInputPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort<DeviceCandidate = DiscoveredDevice>,
{
    fn read_pin(&self) -> Result<zeroize::Zeroizing<Vec<u8>>> {
        self.secret_io.read_pin()
    }
}

impl<D> SecretInputPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort<DeviceCandidate = DiscoveredDevice>,
{
    fn read_visible_secret(&self, label: &str) -> Result<zeroize::Zeroizing<Vec<u8>>> {
        self.secret_io.read_visible_secret(label)
    }

    fn read_hidden_secret(&self, label: &str) -> Result<zeroize::Zeroizing<Vec<u8>>> {
        self.secret_io.read_hidden_secret(label)
    }

    fn read_stdin_secret(&self) -> Result<zeroize::Zeroizing<Vec<u8>>> {
        self.secret_io.read_stdin_secret()
    }

    fn read_secret_document_noninteractive(&self) -> Result<zeroize::Zeroizing<Vec<u8>>> {
        self.secret_io.read_secret_document_noninteractive()
    }

    fn read_bootstrap_secret_document(&self) -> Result<BootstrapSecretDocument> {
        self.secret_io.read_bootstrap_secret_document()
    }
}

impl<D> SecretOutputPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort<DeviceCandidate = DiscoveredDevice>,
{
    fn write_secret(&self, bytes: &[u8]) -> Result<()> {
        self.secret_io.write_secret(bytes)
    }
}

impl<D> SecretLoadPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort<DeviceCandidate = DiscoveredDevice>,
    D::Device: SecretDeviceExt,
{
    fn load_secret(
        &mut self,
        serial: u32,
        name: SecretName,
    ) -> Result<zeroize::Zeroizing<Vec<u8>>> {
        self.with_verified_device(serial, |device, _| device.load_secret(name))
    }
}

impl<D> SecretStorePort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort<DeviceCandidate = DiscoveredDevice>,
    D::Device: SecretDeviceExt,
{
    fn store_secret(
        &mut self,
        serial: u32,
        name: SecretName,
        force: bool,
        secret: &[u8],
    ) -> Result<()> {
        self.with_device(serial, |device, boundary| {
            device.store_secret(boundary, name, secret, force)
        })
    }
}

impl<D> StorageSetupPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort<DeviceCandidate = DiscoveredDevice>,
    D::Device: SecretDeviceExt,
{
    fn setup_storage(&mut self, serial: u32) -> Result<()> {
        self.with_device(serial, |device, _| device.setup_storage())
    }
}

impl<D> BootstrapSecretLoadPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort<DeviceCandidate = DiscoveredDevice>,
    D::Device: SecretDeviceExt,
{
    fn load_bootstrap_secret_document(&mut self, serial: u32) -> Result<BootstrapSecretDocument> {
        self.with_verified_device(serial, |device, _| {
            let bw_email = device.load_secret(SecretName::BwEmail)?;
            let bw_password = device.load_secret(SecretName::BwPassword)?;
            let bws_access_token = device.load_secret(SecretName::BwsAccessToken)?;
            BootstrapSecretDocument::from_interactive_secrets(
                bw_email.as_ref(),
                bw_password.as_ref(),
                bws_access_token.as_ref(),
            )
        })
    }
}

impl<D> BootstrapSecretStorePort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort<DeviceCandidate = DiscoveredDevice>,
    D::Device: SecretDeviceExt,
{
    fn store_bootstrap_secret_document(
        &mut self,
        serial: u32,
        document: &BootstrapSecretDocument,
    ) -> Result<()> {
        self.with_device(serial, |device, boundary| {
            device.store_bootstrap_secret_document(boundary, document)
        })
    }
}

impl<D> StorageVerifyPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort<DeviceCandidate = DiscoveredDevice>,
    D::Device: SecretDeviceExt,
{
    fn verify_local_storage(&mut self, serial: u32) -> Result<()> {
        self.with_verified_device(serial, |device, _| device.verify_required_secrets())
    }
}

impl<D> ReportPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort<DeviceCandidate = DiscoveredDevice>,
{
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()> {
        self.report.write_enroll_report(summary)
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        self.report.write_verify_report(summary)
    }

    fn report_primary_enrollment(&self, serial: u32) -> Result<()> {
        self.write_enroll_report(&EnrollSummary::primary_completed(serial))
    }

    fn report_spare_enrollment(&self, serial: u32) -> Result<()> {
        self.write_enroll_report(&EnrollSummary::spare_completed(serial))
    }

    fn report_local_storage_verified(&self, serial: u32) -> Result<()> {
        self.write_verify_report(&VerifySummary::local_storage_verified(serial))
    }

    fn report_local_storage_failed(&self, serial: u32) -> Result<()> {
        self.write_verify_report(&VerifySummary::local_storage_failed(serial))
    }

    fn report_external_checks_unavailable(
        &self,
        serial: u32,
        checks: impl IntoIterator<Item = CheckName>,
    ) -> Result<()> {
        self.write_verify_report(&VerifySummary::external_checks_unavailable(serial, checks))
    }
}

impl<D> RandomBytesPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort<DeviceCandidate = DiscoveredDevice>,
{
    fn fill_random_bytes(&self, out: &mut [u8]) -> Result<()> {
        use rand::RngCore;
        rand::rng().fill_bytes(out);
        Ok(())
    }
}
