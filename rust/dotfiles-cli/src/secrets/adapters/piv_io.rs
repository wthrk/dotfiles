//! YubiKey PIV discovery/selection と実プロセス I/O を port 契約へ接続する adapter。

mod device;
#[cfg(feature = "secrets-test-stub")]
mod device_test_stub;
mod report;
mod secret_io;

use anyhow::bail;

use crate::{
    Result,
    secrets::domain::{
        manifest::BootstrapSecretDocument,
        material::SecretMaterial,
        piv::SecretName,
        values::{EnrollSummary, VerifySummary},
    },
    secrets::ports::{
        BootstrapSecretDocumentInputPort, DeviceCandidate, DevicePinPolicyPort,
        DeviceSelectionPort, DeviceSerialPort, PinInputPort, RandomBytesPort, ReportPort,
        SecretDevice, SecretInputPort, SecretOutputPort, SpareDeviceSerialPort,
    },
};

use self::{
    device::SelectedDeviceAdapter, report::JsonReportAdapter, secret_io::RealSecretIoAdapter,
};

/// 実機 device・実プロセス I/O・report 出力を束ねる runtime adapter。
///
/// この型は複数 port の実装を 1 箇所に集約し、application 層へ concrete I/O を漏らさない境界として機能する。
pub(crate) struct RealSecretsBoundary<D = SelectedDeviceAdapter>
where
    D: DeviceSelectionPort,
{
    device: D,
    secret_io: RealSecretIoAdapter,
    report: JsonReportAdapter,
}

impl<D> Default for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort + Default,
{
    fn default() -> Self {
        Self {
            device: D::default(),
            secret_io: RealSecretIoAdapter,
            report: JsonReportAdapter,
        }
    }
}

impl<D> DeviceSelectionPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
    D::Device: SecretDevice,
{
    type Device = D::Device;

    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>> {
        self.device.discover_devices()
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<Self::Device> {
        self.device.open_device_by_serial(serial)
    }
}

impl<D> DeviceSerialPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
{
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
        if let Some(serial) = requested {
            return Ok(serial);
        }
        let devices = self.discover_devices()?;
        match devices.as_slice() {
            [] => bail!("no YubiKey detected"),
            [device] => Ok(device.serial),
            _ => bail!("multiple YubiKeys detected; pass --serial to select a device"),
        }
    }
}

impl<D> SpareDeviceSerialPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
{
    fn resolve_spare_device_serial(
        &mut self,
        primary_serial: Option<u32>,
        requested_spare_serial: Option<u32>,
    ) -> Result<u32> {
        let spare_serial = self.resolve_device_serial(requested_spare_serial)?;
        if primary_serial == Some(spare_serial) {
            bail!("primary and spare YubiKey serial must be different");
        }
        Ok(spare_serial)
    }
}

impl<D> DevicePinPolicyPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
    D::Device: SecretDevice,
{
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool> {
        let device = self.open_device_by_serial(serial)?;
        Ok(device.requires_pin_input())
    }
}

impl<D> PinInputPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
{
    fn read_pin(&self) -> Result<SecretMaterial> {
        self.secret_io.read_pin()
    }
}

impl<D> SecretInputPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
{
    fn read_visible_secret(&self) -> Result<SecretMaterial> {
        self.secret_io.read_visible_secret()
    }

    fn read_hidden_secret(&self, name: SecretName) -> Result<SecretMaterial> {
        self.secret_io.read_hidden_secret(name)
    }

    fn read_stdin_secret(&self) -> Result<SecretMaterial> {
        self.secret_io.read_stdin_secret()
    }
}

impl<D> BootstrapSecretDocumentInputPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
{
    fn read_bootstrap_secret_document_noninteractive(&self) -> Result<BootstrapSecretDocument> {
        self.secret_io
            .read_bootstrap_secret_document_noninteractive()
    }
}

impl<D> SecretOutputPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
{
    fn write_secret(&self, secret: &SecretMaterial) -> Result<()> {
        self.secret_io.write_secret(secret)
    }
}

impl ReportPort for RealSecretsBoundary<SelectedDeviceAdapter> {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()> {
        self.report
            .write_enroll_report_for_route(summary, self.device.adapter_route_label())
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        self.report
            .write_verify_report_for_route(summary, self.device.adapter_route_label())
    }
}

impl<D> RandomBytesPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
{
    fn fill_random_bytes(&self, out: &mut [u8]) -> Result<()> {
        use rand::RngCore;
        rand::rng().fill_bytes(out);
        Ok(())
    }
}
