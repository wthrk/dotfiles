//! YubiKey PIV discovery/selection と実プロセス I/O を port 契約へ接続する adapter。

mod report;
mod secret_io;

use anyhow::{Context, bail};
use rsa::{RsaPublicKey, pkcs1::DecodeRsaPublicKey};
use yubikey::{
    Context as YubikeyContext, MgmKey, PinPolicy, Serial, TouchPolicy, Version, YubiKey,
    piv::{self, AlgorithmId, RetiredSlotId, SlotId},
};

use crate::{
    Result,
    secrets::{
        domain::{
            manifest::BootstrapSecretDocument,
            material::SecretMaterial,
            piv::{PivObjectId, SecretName, SecretStorageSpec},
            storage::{
                SecretStorageReadInspection, SecretStorageReadIntent, SecretStorageSetupInspection,
                SecretStorageSetupIntent, SecretStorageSetupProbe, SecretStorageWriteInspection,
                SecretStorageWriteIntent,
            },
        },
        ports::{
            BootstrapSecretDocumentInputPort, DevicePinPolicyPort, DeviceSerialPort, PinInputPort,
            SecretInputPort, SecretOutputPort, SecretStoragePort, SpareDeviceSerialPort,
        },
        support::protection::{sealed_blob, secret_consumer, secret_random},
    },
};

use self::secret_io::RealSecretIoAdapter;

const SECRET_SLOT: SlotId = SlotId::Retired(RetiredSlotId::R1);
const SECRET_SLOT_CERT_OBJECT_ID: u32 = 0x005f_c10d;
const MIN_PIV_METADATA_VERSION: Version = Version {
    major: 5,
    minor: 3,
    patch: 0,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeviceCandidate {
    serial: u32,
    label: String,
}

trait RealDeviceIo {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>>;
    fn open_device_by_serial(&mut self, serial: u32) -> Result<YubikeySecretDevice>;
}

trait SecretDeviceIo {
    fn key_exists(&mut self) -> Result<bool>;
    fn check_key_generation_preconditions(&mut self) -> Result<()>;
    fn check_management_auth_preconditions(&mut self) -> Result<()>;
    fn generate_key(&mut self) -> Result<()>;
    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>>;
    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()>;
    fn requires_pin_input(&self) -> bool;
    fn verify_pin(&mut self, pin: &SecretMaterial) -> Result<()>;
    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        plaintext: &SecretMaterial,
    ) -> Result<Vec<u8>>;
    fn open_from_storage(
        &mut self,
        storage: SecretStorageSpec,
        encoded: &[u8],
    ) -> Result<SecretMaterial>;
}

trait SelectedDeviceDiscoveryIo {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>>;
    fn open_device_by_serial(&mut self, serial: u32) -> Result<SelectedSecretDevice>;
}

/// device serial 解決と PIN 要否判定を port 契約へ翻訳する adapter。
#[derive(Default)]
pub(crate) struct DeviceSelectionAdapter {
    device: SelectedDeviceAdapter,
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

const ADAPTER_ROUTE_AUDIT_PREFIX: &str = "DOTFILES_SECRETS_DEVICE_ADAPTER_ROUTE";

/// 同一 production command path 上で device 選択 route を確定する adapter。
///
/// production 経路は実機 `real` route 固定で、feature/env による runtime 差し替えを持たない。
struct SelectedDeviceAdapter {
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

enum SelectedSecretDevice {
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

struct YubikeySecretDevice {
    yubikey: YubiKey,
    pin_verified: bool,
}

impl YubikeySecretDevice {
    fn open_by_serial(serial: u32) -> Result<Self> {
        Ok(Self {
            yubikey: YubiKey::open_by_serial(Serial(serial))?,
            pin_verified: false,
        })
    }

    fn default_management_key(&self) -> Result<MgmKey> {
        // Current phase assumes the factory-default management key; repository-specific
        // non-default management-key handling is deferred to a later phase.
        MgmKey::get_default(&self.yubikey).context("failed to load default YubiKey management key")
    }

    fn is_version_below_minimum(&self) -> bool {
        let version = self.yubikey.version();
        (version.major, version.minor, version.patch)
            < (
                MIN_PIV_METADATA_VERSION.major,
                MIN_PIV_METADATA_VERSION.minor,
                MIN_PIV_METADATA_VERSION.patch,
            )
    }

    fn minimum_version_string() -> String {
        format!(
            "{}.{}.{}",
            MIN_PIV_METADATA_VERSION.major,
            MIN_PIV_METADATA_VERSION.minor,
            MIN_PIV_METADATA_VERSION.patch
        )
    }

    fn wrap_content_key(&mut self, key: &SecretMaterial) -> Result<Vec<u8>> {
        let metadata = piv::metadata(&mut self.yubikey, SECRET_SLOT)?;
        let public = metadata
            .public
            .context("YubiKey secret storage key has no public key metadata")?;
        let public = RsaPublicKey::from_pkcs1_der(public.subject_public_key.raw_bytes())
            .context("failed to parse YubiKey secret storage public key")?;
        secret_random::rsa_oaep_encrypt(&public, key)
    }

    fn unwrap_content_key(&mut self, wrapped_key: &[u8]) -> Result<SecretMaterial> {
        if !self.pin_verified {
            bail!("YubiKey PIN must be verified before reading stored secrets");
        }
        let decrypted = piv::decrypt_data(
            &mut self.yubikey,
            wrapped_key,
            AlgorithmId::Rsa2048,
            SECRET_SLOT,
        )?;
        sealed_blob::unwrap_content_key(&decrypted, 256)
    }
}

struct RealDeviceAdapter;

impl RealDeviceIo for RealDeviceAdapter {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>> {
        let mut context = YubikeyContext::open()?;
        let mut devices = Vec::new();
        for reader in context.iter()? {
            let label = reader.name().into_owned();
            let yubikey = reader.open()?;
            devices.push(DeviceCandidate {
                serial: yubikey.serial().0,
                label,
            });
        }
        Ok(devices)
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<YubikeySecretDevice> {
        YubikeySecretDevice::open_by_serial(serial)
    }
}

impl SecretDeviceIo for YubikeySecretDevice {
    fn key_exists(&mut self) -> Result<bool> {
        match piv::metadata(&mut self.yubikey, SECRET_SLOT) {
            Ok(_) => Ok(true),
            Err(yubikey::Error::NotFound) => {
                match self.yubikey.fetch_object(SECRET_SLOT_CERT_OBJECT_ID) {
                    Ok(_) => Ok(true),
                    Err(yubikey::Error::NotFound) => Ok(false),
                    Err(err) => Err(err.into()),
                }
            }
            Err(err) => Err(err.into()),
        }
    }

    fn check_key_generation_preconditions(&mut self) -> Result<()> {
        if self.is_version_below_minimum() {
            bail!(
                "YubiKey PIV application version must be at least {}",
                Self::minimum_version_string()
            );
        }
        if self.yubikey.get_pin_retries()? == 0 {
            bail!("YubiKey PIN retries are exhausted");
        }
        Ok(())
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        let key = self.default_management_key()?;
        self.yubikey.authenticate(&key)?;
        Ok(())
    }

    fn generate_key(&mut self) -> Result<()> {
        self.check_key_generation_preconditions()?;
        let key = self.default_management_key()?;
        self.yubikey.authenticate(&key)?;
        piv::generate(
            &mut self.yubikey,
            SECRET_SLOT,
            AlgorithmId::Rsa2048,
            PinPolicy::Once,
            TouchPolicy::Always,
        )?;
        Ok(())
    }

    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>> {
        match self.yubikey.fetch_object(object_id.value()) {
            Ok(value) => Ok(Some(value.to_vec())),
            Err(yubikey::Error::NotFound) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()> {
        let key = self.default_management_key()?;
        self.yubikey.authenticate(&key)?;
        self.yubikey.save_object(object_id.value(), value)?;
        Ok(())
    }

    fn verify_pin(&mut self, pin: &SecretMaterial) -> Result<()> {
        if self.pin_verified {
            return Ok(());
        }
        secret_consumer::consume(pin, &mut YubikeyPinVerifier(&mut self.yubikey))?;
        self.pin_verified = true;
        Ok(())
    }

    fn requires_pin_input(&self) -> bool {
        !self.pin_verified
    }

    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        plaintext: &SecretMaterial,
    ) -> Result<Vec<u8>> {
        sealed_blob::seal_with_key_wrap(
            sealed_blob::SealWithKeyWrapRequest {
                secret_id: storage.secret_id,
                plaintext,
                aad: &storage.additional_data,
                minimum_plaintext_len: storage.minimum_plaintext_len,
                label: &storage.label,
            },
            |content_key| self.wrap_content_key(content_key),
        )
    }

    fn open_from_storage(
        &mut self,
        storage: SecretStorageSpec,
        encoded: &[u8],
    ) -> Result<SecretMaterial> {
        sealed_blob::open_with_key_unwrap(
            encoded,
            storage.secret_id,
            |wrapped_key| self.unwrap_content_key(wrapped_key),
            &storage.additional_data,
            storage.minimum_plaintext_len,
            &storage.label,
        )
    }
}

struct YubikeyPinVerifier<'a>(&'a mut YubiKey);

impl secret_consumer::SecretConsumer for YubikeyPinVerifier<'_> {
    fn consume(&mut self, bytes: &[u8]) -> Result<()> {
        self.0.verify_pin(bytes).map_err(anyhow::Error::new)
    }
}
