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
        manifest::{BootstrapSecretDocument, SecretManifest},
        material::SecretMaterial,
        piv::{PivObjectId, SecretName, SecretStorageSpec, StorageObjectIds},
    },
    secrets::ports::{
        BootstrapSecretDocumentInputPort, DeviceCandidate, DevicePinPolicyPort, DeviceSerialPort,
        PinInputPort, SecretInputPort, SecretOutputPort, SecretStoragePort, SpareDeviceSerialPort,
    },
};

use self::{device::SelectedDeviceAdapter, secret_io::RealSecretIoAdapter};
use super::SecretDeviceIo;

trait SelectedDeviceDiscoveryIo {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>>;
    fn open_device_by_serial(&mut self, serial: u32) -> Result<device::SelectedSecretDevice>;
}

#[cfg(feature = "secrets-test-stub")]
trait StubDeviceDiscoveryIo {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>>;
    fn open_device_by_serial(&mut self, serial: u32) -> Result<device::SelectedSecretDevice>;
}

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

impl RealSecretsBoundary {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>> {
        SelectedDeviceDiscoveryIo::discover_devices(&mut self.device)
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<device::SelectedSecretDevice> {
        SelectedDeviceDiscoveryIo::open_device_by_serial(&mut self.device, serial)
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

impl SecretStoragePort for RealSecretsBoundary {
    fn initialize_secret_storage(&mut self, serial: u32) -> Result<()> {
        let mut device = self.open_device_by_serial(serial)?;
        device.check_key_generation_preconditions()?;
        device.check_management_auth_preconditions()?;
        let key_exists = device.key_exists()?;
        let manifest_bytes = device.read_object(PivObjectId::MANIFEST)?;
        let mut occupied_object_ids = Vec::new();
        for object_id in StorageObjectIds::iter() {
            if device.read_object(object_id)?.is_some() {
                occupied_object_ids.push(object_id);
            }
        }
        SecretManifest::ensure_setup_allowed(
            key_exists,
            manifest_bytes.as_deref(),
            &occupied_object_ids,
        )?;
        device.generate_key()?;
        let mut manifest = SecretManifest::expected().encode()?;
        device.write_object(PivObjectId::MANIFEST, &mut manifest)
    }

    fn store_secret(
        &mut self,
        serial: u32,
        storage: SecretStorageSpec,
        secret: &SecretMaterial,
    ) -> Result<()> {
        let mut device = self.open_device_by_serial(serial)?;
        SecretManifest::decode_initialized(device.read_object(PivObjectId::MANIFEST)?.as_deref())?;
        device.check_management_auth_preconditions()?;
        let mut encoded = device.seal_for_storage(storage.clone(), secret)?;
        device.write_object(storage.object_id, &mut encoded)
    }

    fn put_secret(
        &mut self,
        serial: u32,
        storage: SecretStorageSpec,
        secret: &SecretMaterial,
        force: bool,
    ) -> Result<()> {
        let mut device = self.open_device_by_serial(serial)?;
        SecretManifest::decode_initialized(device.read_object(PivObjectId::MANIFEST)?.as_deref())?;
        device.check_management_auth_preconditions()?;
        storage
            .name
            .ensure_write_allowed(device.read_object(storage.object_id)?.is_some(), force)?;
        let mut encoded = device.seal_for_storage(storage.clone(), secret)?;
        device.write_object(storage.object_id, &mut encoded)
    }

    fn load_secret(
        &mut self,
        serial: u32,
        storage: SecretStorageSpec,
        pin: Option<&SecretMaterial>,
    ) -> Result<SecretMaterial> {
        let mut device = self.open_device_by_serial(serial)?;
        if device.requires_pin_input() {
            let Some(pin) = pin else {
                bail!("PIN is required for this operation");
            };
            device.verify_pin(pin)?;
        }
        SecretManifest::decode_initialized(device.read_object(PivObjectId::MANIFEST)?.as_deref())?;
        let encoded = device
            .read_object(storage.object_id)?
            .ok_or_else(|| storage.missing_error())?;
        device
            .open_from_storage(storage.clone(), &encoded)
            .map_err(|error| storage.decode_error(error))
    }

    fn verify_local_storage(&mut self, serial: u32, pin: Option<&SecretMaterial>) -> Result<()> {
        let mut device = self.open_device_by_serial(serial)?;
        if device.requires_pin_input() {
            let Some(pin) = pin else {
                bail!("PIN is required for this operation");
            };
            device.verify_pin(pin)?;
        }
        SecretManifest::decode_initialized(device.read_object(PivObjectId::MANIFEST)?.as_deref())?;
        for storage in SecretStorageSpec::all_for_serial(serial) {
            let encoded = device
                .read_object(storage.object_id)?
                .ok_or_else(|| storage.missing_error())?;
            let _secret = device
                .open_from_storage(storage.clone(), &encoded)
                .map_err(|error| storage.decode_error(error))?;
        }
        Ok(())
    }
}
