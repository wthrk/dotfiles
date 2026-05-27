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
        manifest::BootstrapSecretDocument, material::SecretMaterial, piv::SecretName,
    },
    secrets::ports::{
        BootstrapSecretDocumentInputPort, DeviceCandidate, DevicePinPolicyPort,
        DeviceSelectionPort, DeviceSerialPort, PinInputPort, SecretDevice, SecretInputPort,
        SecretOutputPort, SpareDeviceSerialPort,
    },
};

use self::{device::SelectedDeviceAdapter, secret_io::RealSecretIoAdapter};

trait DeviceAdapterRouteLabel {
    fn adapter_route_label(&self) -> &'static str;
}

/// 実機 device・実プロセス I/O・report 出力を束ねる runtime adapter。
///
/// この型は複数 port の実装を 1 箇所に集約し、application 層へ concrete I/O を漏らさない境界として機能する。
pub(crate) struct RealSecretsBoundary {
    device: SelectedDeviceAdapter,
    secret_io: RealSecretIoAdapter,
    route: &'static str,
}

impl Default for RealSecretsBoundary {
    fn default() -> Self {
        let device = SelectedDeviceAdapter::default();
        let route = device.adapter_route_label();
        Self {
            device,
            secret_io: RealSecretIoAdapter,
            route,
        }
    }
}

impl DeviceSelectionPort for RealSecretsBoundary {
    type Device = <SelectedDeviceAdapter as DeviceSelectionPort>::Device;

    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>> {
        self.device.discover_devices()
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<Self::Device> {
        self.device.open_device_by_serial(serial)
    }
}

impl DeviceSerialPort for RealSecretsBoundary {
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

impl SpareDeviceSerialPort for RealSecretsBoundary {
    fn resolve_spare_device_serial(&mut self, requested_spare_serial: Option<u32>) -> Result<u32> {
        self.resolve_device_serial(requested_spare_serial)
    }
}

impl DevicePinPolicyPort for RealSecretsBoundary {
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool> {
        let device = self.open_device_by_serial(serial)?;
        Ok(device.requires_pin_input())
    }
}

impl PinInputPort for RealSecretsBoundary {
    fn read_pin(&self) -> Result<SecretMaterial> {
        self.secret_io.read_pin()
    }
}

impl SecretInputPort for RealSecretsBoundary {
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

impl BootstrapSecretDocumentInputPort for RealSecretsBoundary {
    fn read_bootstrap_secret_document_noninteractive(&self) -> Result<BootstrapSecretDocument> {
        self.secret_io
            .read_bootstrap_secret_document_noninteractive()
    }
}

impl SecretOutputPort for RealSecretsBoundary {
    fn write_secret(&self, secret: &SecretMaterial) -> Result<()> {
        self.secret_io.write_secret(secret)
    }
}
