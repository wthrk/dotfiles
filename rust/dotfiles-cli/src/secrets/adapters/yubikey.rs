//! 実機 YubiKey PIV セッションを `SecretDevice` port へ接続する adapter。

use anyhow::{Context, bail};
use rsa::{RsaPublicKey, pkcs1::DecodeRsaPublicKey};
use yubikey::{
    Context as YubikeyContext, MgmKey, PinPolicy, Serial, TouchPolicy, Version, YubiKey,
    piv::{self, AlgorithmId, RetiredSlotId, SlotId},
};

use crate::Result;
use crate::secrets::{
    domain::{
        material::SecretMaterial,
        piv::{PivObjectId, SecretStorageSpec},
    },
    ports::{DeviceCandidate, SecretDevice},
    support::protection::{sealed_blob, secret_random, yubikey_pin},
};

const SECRET_SLOT: SlotId = SlotId::Retired(RetiredSlotId::R1);
const SECRET_SLOT_CERT_OBJECT_ID: u32 = 0x005f_c10d;
const MIN_PIV_METADATA_VERSION: Version = Version {
    major: 5,
    minor: 3,
    patch: 0,
};

/// 開いた YubiKey PIV session と PIN 検証状態を保持する実機 adapter。
pub struct YubikeySecretDevice {
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
}

/// 実機 YubiKey discovery/open を `DeviceSelectionPort` 契約へ接続する adapter。
pub(crate) struct RealDeviceAdapter;

impl Default for RealDeviceAdapter {
    fn default() -> Self {
        Self
    }
}

impl crate::secrets::ports::DeviceSelectionPort for RealDeviceAdapter {
    type Device = YubikeySecretDevice;

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

    fn open_device_by_serial(&mut self, serial: u32) -> Result<Self::Device> {
        YubikeySecretDevice::open_by_serial(serial)
    }
}

impl SecretDevice for YubikeySecretDevice {
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

    fn wrap_key(&mut self, key: &SecretMaterial) -> Result<Vec<u8>> {
        let metadata = piv::metadata(&mut self.yubikey, SECRET_SLOT)?;
        let public = metadata
            .public
            .context("YubiKey secret storage key has no public key metadata")?;
        let public = RsaPublicKey::from_pkcs1_der(public.subject_public_key.raw_bytes())
            .context("failed to parse YubiKey secret storage public key")?;
        secret_random::rsa_oaep_encrypt(&public, key)
    }

    fn verify_pin(&mut self, pin: &SecretMaterial) -> Result<()> {
        if self.pin_verified {
            return Ok(());
        }
        yubikey_pin::verify(&mut self.yubikey, pin)?;
        self.pin_verified = true;
        Ok(())
    }

    fn requires_pin_input(&self) -> bool {
        !self.pin_verified
    }

    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<SecretMaterial> {
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

    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        plaintext: &SecretMaterial,
    ) -> Result<Vec<u8>> {
        use rand::RngCore;
        let content_key = secret_random::random_secret(sealed_blob::CONTENT_KEY_LEN)?;
        let mut nonce = [0u8; sealed_blob::NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce);
        let wrapped_key = self.wrap_key(&content_key)?;
        sealed_blob::seal(sealed_blob::SealRequest {
            secret_id: storage.secret_id,
            nonce,
            wrapped_key,
            plaintext,
            content_key: &content_key,
            aad: &storage.additional_data,
            minimum_plaintext_len: storage.minimum_plaintext_len,
            label: &storage.label,
        })
    }

    fn open_from_storage(
        &mut self,
        storage: SecretStorageSpec,
        encoded: &[u8],
    ) -> Result<SecretMaterial> {
        let wrapped_key = sealed_blob::wrapped_key_from_blob(encoded, storage.secret_id)?;
        let content_key = self.unwrap_key(&wrapped_key)?;
        let secret = sealed_blob::open(
            encoded,
            storage.secret_id,
            &content_key,
            &storage.additional_data,
            storage.minimum_plaintext_len,
            &storage.label,
        )?;
        Ok(secret)
    }
}
