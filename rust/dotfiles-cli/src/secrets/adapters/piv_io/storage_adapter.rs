//! YubiKey object I/O と `SecretStoragePort` 契約を接続する storage adapter。
//!
//! manifest/object 読み書きと暗号化 payload の受け渡しを担当し、use case 分岐は保持しない。

use crate::{
    Result,
    secrets::{
        domain::{
            material::SecretMaterial,
            piv::{PivObjectId, SecretStorageSpec},
            storage::{
                SecretStorageReadInspection, SecretStorageReadIntent, SecretStorageSetupInspection,
                SecretStorageSetupIntent, SecretStorageSetupProbe, SecretStorageWriteInspection,
                SecretStorageWriteIntent,
            },
        },
        ports::SecretStoragePort,
    },
};

use super::{
    SecretDeviceIo, SelectedDeviceAdapter, SelectedDeviceDiscoveryIo, SelectedSecretDevice,
};

#[derive(Default)]
pub(crate) struct StorageAdapter {
    device: SelectedDeviceAdapter,
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
        let piv_version = device.piv_application_version();
        let pin_retries = device.pin_retries()?;
        let manifest_bytes = device.read_object(PivObjectId::MANIFEST)?;
        let mut occupied_object_ids = Vec::new();
        for object_id in probe.object_ids() {
            if device.read_object(*object_id)?.is_some() {
                occupied_object_ids.push(*object_id);
            }
        }
        Ok(SecretStorageSetupInspection {
            key_exists,
            piv_version,
            pin_retries,
            manifest_bytes,
            occupied_object_ids,
        })
    }

    fn initialize_secret_storage(
        &mut self,
        serial: u32,
        intent: SecretStorageSetupIntent,
    ) -> Result<()> {
        let mut device = self.open_device_by_serial(serial)?;
        device.check_management_auth_preconditions()?;
        if intent.key_generation_required {
            device.generate_key()?;
        }
        Ok(())
    }

    fn finalize_secret_storage_setup(
        &mut self,
        serial: u32,
        mut intent: SecretStorageSetupIntent,
    ) -> Result<()> {
        let mut device = self.open_device_by_serial(serial)?;
        device.check_management_auth_preconditions()?;
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
        intent: &SecretStorageReadIntent,
        pin: Option<&SecretMaterial>,
    ) -> Result<SecretMaterial> {
        let mut device = self.open_device_by_serial(serial)?;
        if device.requires_pin_input() {
            let Some(pin) = pin else {
                anyhow::bail!("PIN is required for this operation");
            };
            device.verify_pin(pin)?;
        }
        device.open_from_storage(intent.storage.clone(), &intent.encoded)
    }
}
