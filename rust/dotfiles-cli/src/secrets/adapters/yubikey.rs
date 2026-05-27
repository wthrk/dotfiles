//! 実機 YubiKey PIV セッションを secrets adapter 内部の I/O 境界へ接続する。

use anyhow::{Context, bail};
use rsa::{RsaPublicKey, pkcs1::DecodeRsaPublicKey};
use yubikey::{
    Context as YubikeyContext, MgmKey, PinPolicy, Serial, TouchPolicy, Version, YubiKey,
    piv::{self, AlgorithmId, RetiredSlotId, SlotId},
};

use crate::Result;
use crate::secrets::{
    adapters::DeviceCandidate,
    domain::{
        material::SecretMaterial,
        piv::{PivObjectId, SecretStorageSpec},
    },
    support::protection::{sealed_blob, secret_consumer, secret_random},
};

use super::{RealDeviceIo, SecretDeviceIo};

const SECRET_SLOT: SlotId = SlotId::Retired(RetiredSlotId::R1);
const SECRET_SLOT_CERT_OBJECT_ID: u32 = 0x005f_c10d;
const MIN_PIV_METADATA_VERSION: Version = Version {
    major: 5,
    minor: 3,
    patch: 0,
};

/// 開いた YubiKey PIV session と PIN 検証状態を保持する実機 adapter。
pub(crate) struct YubikeySecretDevice {
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

/// 実機 YubiKey discovery/open を担う adapter。
pub(crate) struct RealDeviceAdapter;

impl Default for RealDeviceAdapter {
    fn default() -> Self {
        Self
    }
}

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
        let secret = sealed_blob::open_with_key_unwrap(
            encoded,
            storage.secret_id,
            |wrapped_key| self.unwrap_content_key(wrapped_key),
            &storage.additional_data,
            storage.minimum_plaintext_len,
            &storage.label,
        )?;
        Ok(secret)
    }
}

struct YubikeyPinVerifier<'a>(&'a mut YubiKey);

impl secret_consumer::SecretConsumer for YubikeyPinVerifier<'_> {
    fn consume(&mut self, bytes: &[u8]) -> Result<()> {
        self.0.verify_pin(bytes).map_err(anyhow::Error::new)
    }
}
