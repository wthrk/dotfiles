//! YubiKey PIV discovery/selection と実プロセス I/O を port 契約へ接続する adapter。

mod report;
mod secret_io;

use anyhow::bail;

use crate::{
    Result,
    secrets::domain::{
        manifest::BootstrapSecretDocument,
        material::SecretMaterial,
        piv::{PivObjectId, SecretName, SecretStorageSpec},
        storage::{
            SecretStorageReadInspection, SecretStorageReadIntent, SecretStorageSetupInspection,
            SecretStorageSetupIntent, SecretStorageSetupProbe, SecretStorageWriteInspection,
            SecretStorageWriteIntent,
        },
    },
    secrets::ports::{
        BootstrapSecretDocumentInputPort, DevicePinPolicyPort, DeviceSerialPort,
        PinInputPort, SecretInputPort, SecretOutputPort, SecretStoragePort, SpareDeviceSerialPort,
    },
};

use self::secret_io::RealSecretIoAdapter;
use super::{DeviceCandidate, RealDeviceIo, SecretDeviceIo};

trait SelectedDeviceDiscoveryIo {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>>;
    fn open_device_by_serial(&mut self, serial: u32) -> Result<SelectedSecretDevice>;
}

/// device serial 解決と PIN 要否判定を port 契約へ翻訳する adapter。
pub(crate) struct DeviceSelectionAdapter {
    device: SelectedDeviceAdapter,
}

impl Default for DeviceSelectionAdapter {
    fn default() -> Self {
        Self {
            device: SelectedDeviceAdapter::default(),
        }
    }
}

impl DeviceSelectionAdapter {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>> {
        SelectedDeviceDiscoveryIo::discover_devices(&mut self.device)
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<SelectedSecretDevice> {
        SelectedDeviceDiscoveryIo::open_device_by_serial(&mut self.device, serial)
    }
}

impl DeviceSerialPort for DeviceSelectionAdapter {
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

impl SpareDeviceSerialPort for DeviceSelectionAdapter {
    fn resolve_spare_device_serial(&mut self, requested_spare_serial: Option<u32>) -> Result<u32> {
        self.resolve_device_serial(requested_spare_serial)
    }
}

impl DevicePinPolicyPort for DeviceSelectionAdapter {
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool> {
        let device = self.open_device_by_serial(serial)?;
        Ok(device.requires_pin_input())
    }
}

/// process I/O を secret 入出力 port 契約へ翻訳する adapter。
#[derive(Default)]
pub(crate) struct ProcessIoAdapter {
    secret_io: RealSecretIoAdapter,
}

impl PinInputPort for ProcessIoAdapter {
    fn read_pin(&self) -> Result<SecretMaterial> {
        self.secret_io.read_pin()
    }
}

impl SecretInputPort for ProcessIoAdapter {
    fn read_named_secret(&self, name: SecretName) -> Result<SecretMaterial> {
        self.secret_io.read_named_secret(name)
    }

    fn read_streamed_secret(&self) -> Result<SecretMaterial> {
        self.secret_io.read_streamed_secret()
    }
}

impl BootstrapSecretDocumentInputPort for ProcessIoAdapter {
    fn read_bootstrap_secret_document(&self) -> Result<BootstrapSecretDocument> {
        self.secret_io.read_bootstrap_secret_document()
    }
}

impl SecretOutputPort for ProcessIoAdapter {
    fn write_secret(&self, secret: &SecretMaterial) -> Result<()> {
        self.secret_io.write_secret(secret)
    }
}

/// YubiKey object storage を secret storage port 契約へ翻訳する adapter。
pub(crate) struct StorageAdapter {
    device: SelectedDeviceAdapter,
}

impl Default for StorageAdapter {
    fn default() -> Self {
        Self {
            device: SelectedDeviceAdapter::default(),
        }
    }
}

impl StorageAdapter {
    fn open_device_by_serial(&mut self, serial: u32) -> Result<SelectedSecretDevice> {
        SelectedDeviceDiscoveryIo::open_device_by_serial(&mut self.device, serial)
    }
}

impl SecretStoragePort for StorageAdapter {
    fn inspect_secret_storage_setup(
        &mut self,
        serial: u32,
        probe: &SecretStorageSetupProbe,
    ) -> Result<SecretStorageSetupInspection> {
        let mut device = self.open_device_by_serial(serial)?;
        let key_exists = device.key_exists()?;
        let manifest_bytes = device.read_object(PivObjectId::MANIFEST)?;
        let mut occupied_object_ids = Vec::new();
        for object_id in probe.object_ids() {
            if device.read_object(*object_id)?.is_some() {
                occupied_object_ids.push(*object_id);
            }
        }
        Ok(SecretStorageSetupInspection {
            key_exists,
            manifest_bytes,
            occupied_object_ids,
        })
    }

    fn initialize_secret_storage(
        &mut self,
        serial: u32,
        mut intent: SecretStorageSetupIntent,
    ) -> Result<()> {
        let mut device = self.open_device_by_serial(serial)?;
        device.check_key_generation_preconditions()?;
        device.check_management_auth_preconditions()?;
        device.generate_key()?;
        device.write_object(PivObjectId::MANIFEST, &mut intent.manifest_bytes)
    }

    fn inspect_secret_storage_write(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageWriteInspection> {
        let mut device = self.open_device_by_serial(serial)?;
        let manifest_bytes = device.read_object(PivObjectId::MANIFEST)?;
        let object_exists = device.read_object(storage.object_id)?.is_some();
        Ok(SecretStorageWriteInspection {
            manifest_bytes,
            object_exists,
        })
    }

    fn store_secret(
        &mut self,
        serial: u32,
        intent: SecretStorageWriteIntent,
        secret: &SecretMaterial,
    ) -> Result<()> {
        let mut device = self.open_device_by_serial(serial)?;
        device.check_management_auth_preconditions()?;
        let mut encoded = device.seal_for_storage(intent.storage.clone(), secret)?;
        device.write_object(intent.storage.object_id, &mut encoded)
    }

    fn inspect_secret_storage_read(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageReadInspection> {
        let mut device = self.open_device_by_serial(serial)?;
        let manifest_bytes = device.read_object(PivObjectId::MANIFEST)?;
        let encoded = device.read_object(storage.object_id)?;
        Ok(SecretStorageReadInspection {
            manifest_bytes,
            encoded,
        })
    }

    fn load_secret(
        &mut self,
        serial: u32,
        intent: SecretStorageReadIntent,
        pin: Option<&SecretMaterial>,
    ) -> Result<SecretMaterial> {
        let mut device = self.open_device_by_serial(serial)?;
        if device.requires_pin_input() {
            let Some(pin) = pin else {
                bail!("PIN is required for this operation");
            };
            device.verify_pin(pin)?;
        }
        device
            .open_from_storage(intent.storage.clone(), &intent.encoded)
            .map_err(|error| intent.decode_error(error))
    }
}

/// JSON report 出力を report port 契約へ翻訳する adapter。
pub(crate) struct JsonReportAdapter {
    route: &'static str,
}

impl Default for JsonReportAdapter {
    fn default() -> Self {
        Self { route: "real" }
    }
}

// Device selection is kept inside this port-implementation file so adapters/ does not expose a helper module.
use crate::secrets::adapters::yubikey::{RealDeviceAdapter, YubikeySecretDevice};


const ADAPTER_ROUTE_AUDIT_PREFIX: &str = "DOTFILES_SECRETS_DEVICE_ADAPTER_ROUTE";

/// 同一 production command path 上で device 選択 route を確定する adapter。
///
/// production 経路は実機 `real` route 固定で、feature/env による runtime 差し替えを持たない。
pub(crate) struct SelectedDeviceAdapter {
    inner: DeviceSelectionInner,
}

impl Default for SelectedDeviceAdapter {
    fn default() -> Self {
        Self::new()
    }
}

enum DeviceSelectionInner {
    Real(RealDeviceAdapter),
}

pub(crate) enum SelectedSecretDevice {
    Real(YubikeySecretDevice),
}

impl SecretDeviceIo for SelectedSecretDevice {
    fn key_exists(&mut self) -> Result<bool> {
        match self {
            Self::Real(device) => device.key_exists(),
        }
    }

    fn check_key_generation_preconditions(&mut self) -> Result<()> {
        match self {
            Self::Real(device) => device.check_key_generation_preconditions(),
        }
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        match self {
            Self::Real(device) => device.check_management_auth_preconditions(),
        }
    }

    fn generate_key(&mut self) -> Result<()> {
        match self {
            Self::Real(device) => device.generate_key(),
        }
    }

    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>> {
        match self {
            Self::Real(device) => device.read_object(object_id),
        }
    }

    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()> {
        match self {
            Self::Real(device) => device.write_object(object_id, value),
        }
    }

    fn requires_pin_input(&self) -> bool {
        match self {
            Self::Real(device) => device.requires_pin_input(),
        }
    }

    fn verify_pin(&mut self, pin: &SecretMaterial) -> Result<()> {
        match self {
            Self::Real(device) => device.verify_pin(pin),
        }
    }

    fn seal_for_storage(
        &mut self,
        storage: crate::secrets::domain::piv::SecretStorageSpec,
        plaintext: &crate::secrets::domain::material::SecretMaterial,
    ) -> Result<Vec<u8>> {
        match self {
            Self::Real(device) => device.seal_for_storage(storage, plaintext),
        }
    }

    fn open_from_storage(
        &mut self,
        storage: crate::secrets::domain::piv::SecretStorageSpec,
        encoded: &[u8],
    ) -> Result<crate::secrets::domain::material::SecretMaterial> {
        match self {
            Self::Real(device) => device.open_from_storage(storage, encoded),
        }
    }
}

impl SelectedDeviceAdapter {
    /// production command path の device route を実機 `real` に固定する。
    ///
    /// テスト fixture は production route を差し替えず、別の harness 側で扱う。
    fn new() -> Self {
        eprintln!("{ADAPTER_ROUTE_AUDIT_PREFIX}=real");
        Self {
            inner: DeviceSelectionInner::Real(RealDeviceAdapter),
        }
    }
}

impl SelectedDeviceDiscoveryIo for SelectedDeviceAdapter {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>> {
        match &mut self.inner {
            DeviceSelectionInner::Real(inner) => RealDeviceIo::discover_devices(inner),
        }
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<SelectedSecretDevice> {
        match &mut self.inner {
            DeviceSelectionInner::Real(inner) => {
                RealDeviceIo::open_device_by_serial(inner, serial).map(SelectedSecretDevice::Real)
            }
        }
    }
}
