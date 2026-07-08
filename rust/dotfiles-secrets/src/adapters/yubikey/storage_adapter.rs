//! YubiKey object I/O と `SecretStoragePort` 契約を接続する storage adapter。
//!
//! manifest/object 読み書きと暗号化 payload の受け渡しを担当し、use case 分岐は保持しない。

use crate::{
    Result,
    domain::{
        piv::{PivObjectId, SecretStorageSpec},
        storage::{
            SecretStorageReadInspection, SecretStorageReadIntent, SecretStorageSetupInspection,
            SecretStorageSetupIntent, SecretStorageSetupProbe, SecretStorageWriteInspection,
            SecretStorageWriteIntent,
        },
    },
    ports::yubikey::SecretStoragePort,
    support::protection::{ProtectedSecret, SecretSession},
};

use std::collections::BTreeMap;

use super::{
    SecretDeviceIo, SelectedDeviceAdapter, SelectedDeviceDiscoveryIo, SelectedSecretDevice,
};

/// YubiKey PIV object I/O を `SecretStoragePort` の inspection/intent 契約へ翻訳する adapter。
///
/// caller は domain が判定した intent を渡す。adapter は device API、object 読み書き、
/// protection 操作の接続だけを担い、storage plan や上書き可否を再判定しない。
#[derive(Default)]
pub(super) struct StorageAdapter {
    device: SelectedDeviceAdapter,
    generated_public_keys: BTreeMap<u32, Vec<u8>>,
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
            let public_key = device.generate_key()?;
            self.generated_public_keys.insert(serial, public_key);
        }
        Ok(())
    }

    fn finalize_secret_storage_setup(
        &mut self,
        serial: u32,
        mut intent: SecretStorageSetupIntent,
    ) -> Result<()> {
        self.generated_public_keys.remove(&serial);
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
        secret: &ProtectedSecret,
    ) -> Result<()> {
        let mut device = self.open_device_by_serial(serial)?;
        device.check_management_auth_preconditions()?;
        if let Some(public_key) = self.generated_public_keys.get(&serial).cloned() {
            device.remember_generated_public_key(public_key);
        }
        let mut encoded = device.seal_for_storage(intent.storage.clone(), secret)?;
        device.write_object(intent.storage.object_id, &mut encoded)
    }

    fn inspect_secret_storage_read(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageReadInspection> {
        let _session = SecretSession::start()?;
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
        pin: Option<&ProtectedSecret>,
    ) -> Result<ProtectedSecret> {
        let _session = SecretSession::start()?;
        let mut device = self.open_device_by_serial(serial)?;
        if device.requires_pin_input() {
            let Some(pin) = pin else {
                anyhow::bail!("PIN is required for this operation");
            };
            device.verify_pin(pin)?;
        }
        device.open_from_storage(intent.storage.clone(), &intent.encoded)
    }

    fn verify_pin_input(&mut self, serial: u32, pin: &ProtectedSecret) -> Result<()> {
        let _session = SecretSession::start()?;
        let mut device = self.open_device_by_serial(serial)?;
        if !device.requires_pin_input() {
            return Ok(());
        }
        device.verify_pin(pin)
    }
}
