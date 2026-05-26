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
        values::{DeviceCandidate, EnrollSummary, VerifySummary},
    },
    secrets::ports::{
        BootstrapSecretDocumentInputPort, DevicePinPolicyPort, DeviceSelectionPort,
        DeviceSerialPort, PinInputPort, RandomBytesPort, ReportPort, SecretDevice, SecretInputPort,
        SecretLoadPort, SecretOutputPort, SecretStorePort, SpareDeviceSerialPort, StorageSetupPort,
        StorageVerifyPort,
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

/// PIN 必須デバイスに対してのみ `verify_pin` を実行する。
///
/// PIN の取得手段選択（TTY か stdin か）は caller が担い、
/// この helper は「PIN 必須時に入力が無ければ停止する」境界だけを担う。
fn verify_pin_if_required(
    device: &mut impl SecretDevice,
    pin: Option<&SecretMaterial>,
) -> Result<()> {
    // PIN 要求フラグが false の device では、ここで即 return して追加 I/O を行わない。
    // PIN 要求時に `pin` が `None` だった場合の停止はこの関数が担い、
    // 実際の PIN 値取得（TTY / stdin などの境界選択）は caller 側の責務とする。
    if !device.requires_pin_input() {
        return Ok(());
    }
    let Some(pin) = pin else {
        bail!("PIN is required for this operation");
    };
    device.verify_pin(pin)
}

impl<D> RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
    D::Device: SecretDevice,
{
    /// 指定 serial の device から 1 secret を読み出す。
    ///
    /// PIN が必要なデバイスでは検証を先に実施し、未入力時はここで停止する。
    fn load_secret_from_device(
        &mut self,
        serial: u32,
        name: SecretName,
        pin: Option<&SecretMaterial>,
    ) -> Result<SecretMaterial> {
        let mut device = self.open_device_by_serial(serial)?;
        verify_pin_if_required(&mut device, pin)?;
        device.load_secret(name)
    }

    /// 指定 serial の device へ 1 secret を保存する。
    ///
    /// 上書き可否判定は `SecretDevice::store_secret` の契約へ委譲する。
    fn store_secret_to_device(
        &mut self,
        serial: u32,
        name: SecretName,
        force: bool,
        secret: &SecretMaterial,
    ) -> Result<()> {
        let mut device = self.open_device_by_serial(serial)?;
        device.store_secret(self, name, secret, force)
    }

    /// 指定 serial の storage setup を実行する。
    fn setup_storage_on_device(&mut self, serial: u32) -> Result<()> {
        let mut device = self.open_device_by_serial(serial)?;
        device.setup_storage()
    }

    /// 指定 serial の local storage 整合を検証する。
    ///
    /// PIN 必須デバイスでは事前検証を通したうえで必須 secret 群の読み出し確認を行う。
    fn verify_local_storage_on_device(
        &mut self,
        serial: u32,
        pin: Option<&SecretMaterial>,
    ) -> Result<()> {
        let mut device = self.open_device_by_serial(serial)?;
        verify_pin_if_required(&mut device, pin)?;
        device.verify_required_secrets()
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

impl<D> SecretLoadPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
    D::Device: SecretDevice,
{
    fn load_secret(
        &mut self,
        serial: u32,
        name: SecretName,
        pin: Option<&SecretMaterial>,
    ) -> Result<SecretMaterial> {
        self.load_secret_from_device(serial, name, pin)
    }
}

impl<D> SecretStorePort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
    D::Device: SecretDevice,
{
    fn store_secret(
        &mut self,
        serial: u32,
        name: SecretName,
        force: bool,
        secret: &SecretMaterial,
    ) -> Result<()> {
        self.store_secret_to_device(serial, name, force, secret)
    }
}

impl<D> StorageSetupPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
    D::Device: SecretDevice,
{
    fn setup_storage(&mut self, serial: u32) -> Result<()> {
        self.setup_storage_on_device(serial)
    }
}

impl<D> StorageVerifyPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
    D::Device: SecretDevice,
{
    fn verify_local_storage(&mut self, serial: u32, pin: Option<&SecretMaterial>) -> Result<()> {
        self.verify_local_storage_on_device(serial, pin)
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
