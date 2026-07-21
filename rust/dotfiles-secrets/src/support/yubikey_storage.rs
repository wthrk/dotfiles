//! YubiKey PIV storage の technical receiver と I/O sequence。

use crate::{
    Result,
    domain::{
        piv::{PivObjectId, SecretStorageSpec},
        storage::{
            SecretStorageClearIntent, SecretStorageReadInspection, SecretStorageReadIntent,
            SecretStorageSetupInspection, SecretStorageSetupIntent, SecretStorageSetupProbe,
            SecretStorageStatusInspection, SecretStorageWriteInspection, SecretStorageWriteIntent,
        },
    },
    support::{
        piv_storage::non_empty_payload,
        protection::{ProtectedSecret, SecretSession},
        yubikey_backend::{
            self, ManagementAuthState, SecretDeviceIo, SelectedSecretDevice, YubikeyDeviceBackend,
        },
    },
};
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct YubikeyStorageBackend {
    generated_public_keys: BTreeMap<u32, Vec<u8>>,
    piv_management_pin: Option<ProtectedSecret>,
}
fn open_device(serial: u32) -> Result<SelectedSecretDevice> {
    yubikey_backend::open_device_by_serial(&mut YubikeyDeviceBackend, serial)
}
fn open_authenticated(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
) -> Result<SelectedSecretDevice> {
    let pin =
        ProtectedSecret::try_clone(backend.piv_management_pin.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "PIV management operation requires a configured PIN-protected management session"
            )
        })?)?;
    let mut device = open_device(serial)?;
    if device.check_management_auth_preconditions(Some(&pin))? == ManagementAuthState::Bootstrapped
    {
        drop(device);
        let mut fresh = open_device(serial)?;
        if fresh.check_management_auth_preconditions(Some(&pin))? != ManagementAuthState::Protected
        {
            anyhow::bail!(
                "YubiKey PIN-protected management key bootstrap did not become healthy on a fresh handle"
            );
        }
        return Ok(fresh);
    }
    Ok(device)
}
pub(crate) fn begin_piv_management_session(
    backend: &mut YubikeyStorageBackend,
    pin: ProtectedSecret,
) -> Result<()> {
    backend.piv_management_pin = Some(pin);
    Ok(())
}
pub(crate) fn inspect_secret_storage_setup(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    probe: &SecretStorageSetupProbe,
) -> Result<SecretStorageSetupInspection> {
    let mut device = open_authenticated(backend, serial)?;
    let piv_version = device.piv_application_version();
    let manifest_bytes = device.read_object(PivObjectId::MANIFEST)?;
    let slot_public_key_spki = manifest_bytes
        .as_ref()
        .map(|_| device.slot_public_key_spki())
        .transpose()?
        .flatten();
    let key_exists = if slot_public_key_spki.is_some() {
        true
    } else {
        device.key_exists()? || device.reserved_slot_certificate_exists()?
    };
    let mut occupied_object_ids = Vec::new();
    for object_id in probe.object_ids() {
        if device.read_object(*object_id)?.is_some() {
            occupied_object_ids.push(*object_id)
        }
    }
    Ok(SecretStorageSetupInspection {
        key_exists,
        piv_version,
        manifest_bytes,
        occupied_object_ids,
    })
}
pub(crate) fn initialize_secret_storage(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    intent: SecretStorageSetupIntent,
) -> Result<Vec<u8>> {
    let mut device = open_authenticated(backend, serial)?;
    if intent.key_generation_required {
        backend
            .generated_public_keys
            .insert(serial, device.generate_key()?);
    }
    match backend.generated_public_keys.get(&serial).cloned() {
        Some(key) => Ok(key),
        None => device
            .slot_public_key_spki()?
            .ok_or_else(|| anyhow::anyhow!("YubiKey slot 82 public key metadata is unavailable")),
    }
}
pub(crate) fn finalize_secret_storage_setup(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    mut manifest: Vec<u8>,
) -> Result<()> {
    backend.generated_public_keys.remove(&serial);
    let mut device = open_authenticated(backend, serial)?;
    device.write_object(PivObjectId::MANIFEST, &mut manifest)
}
pub(crate) fn clear_secret_storage(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    intent: SecretStorageClearIntent,
) -> Result<Vec<u8>> {
    let mut device = open_authenticated(backend, serial)?;
    for object in &intent.object_ids {
        device.empty_object(*object)?
    }
    device.clear_reserved_slot_certificate()?;
    device.generate_key()
}
pub(crate) fn inspect_secret_storage_write(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    storage: &SecretStorageSpec,
) -> Result<SecretStorageWriteInspection> {
    let mut device = open_authenticated(backend, serial)?;
    let manifest_bytes = device.read_object(PivObjectId::MANIFEST)?;
    let object = device.read_object(storage.object_id)?;
    let object_present = object.is_some();
    let object_exists = non_empty_payload(object).is_some();
    let slot_public_key_spki = manifest_bytes
        .as_ref()
        .map(|_| device.slot_public_key_spki())
        .transpose()?
        .flatten();
    let reserved_slot_key_exists = if slot_public_key_spki.is_some() {
        true
    } else {
        device.key_exists()?
    };
    Ok(SecretStorageWriteInspection {
        manifest_bytes,
        object_present,
        object_exists,
        reserved_slot_key_exists,
        reserved_slot_certificate_exists: device.reserved_slot_certificate_exists()?,
        slot_public_key_spki,
    })
}
pub(crate) fn inspect_secret_storage_status(
    _: &mut YubikeyStorageBackend,
    serial: u32,
    storage: &SecretStorageSpec,
) -> Result<SecretStorageStatusInspection> {
    let mut device = open_device(serial)?;
    let manifest_bytes = device.read_object(PivObjectId::MANIFEST)?;
    let object = device.read_object(storage.object_id)?;
    Ok(SecretStorageStatusInspection {
        manifest_bytes,
        object_present: object.is_some(),
        object_exists: non_empty_payload(object).is_some(),
    })
}
pub(crate) fn store_secret(
    backend: &mut YubikeyStorageBackend,
    serial: u32,
    intent: SecretStorageWriteIntent,
    secret: &ProtectedSecret,
) -> Result<()> {
    let mut device = open_authenticated(backend, serial)?;
    if let Some(key) = intent.slot_public_key_spki {
        device.remember_generated_public_key(key)
    }
    let mut encoded = device.seal_for_storage(intent.storage.clone(), secret)?;
    device.write_object(intent.storage.object_id, &mut encoded)
}
pub(crate) fn inspect_secret_storage_read(
    _: &mut YubikeyStorageBackend,
    serial: u32,
    storage: &SecretStorageSpec,
) -> Result<SecretStorageReadInspection> {
    let _session = SecretSession::start()?;
    let mut device = open_device(serial)?;
    Ok(SecretStorageReadInspection {
        manifest_bytes: device.read_object(PivObjectId::MANIFEST)?,
        encoded: device.read_object(storage.object_id)?,
    })
}
pub(crate) fn load_secret(
    _: &mut YubikeyStorageBackend,
    serial: u32,
    intent: &SecretStorageReadIntent,
) -> Result<ProtectedSecret> {
    let _session = SecretSession::start()?;
    let mut device = open_device(serial)?;
    device.open_from_storage(intent.storage.clone(), &intent.encoded)
}
