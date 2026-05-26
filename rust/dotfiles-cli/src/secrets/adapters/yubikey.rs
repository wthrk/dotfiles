//! 実機 YubiKey PIV セッションを `SecretDevice` port へ接続する adapter。

use anyhow::{Context, bail};
use rand_core::OsRng;
use rsa::{Oaep, RsaPublicKey, pkcs1::DecodeRsaPublicKey};
use sha2::Sha256;
use yubikey::{
    Context as YubikeyContext, MgmKey, PinPolicy, Serial, TouchPolicy, Version, YubiKey,
    piv::{self, AlgorithmId, RetiredSlotId, SlotId},
};
use zeroize::{Zeroize, Zeroizing};

use crate::Result;
use crate::secrets::{
    domain::{material::SecretMaterial, piv::PivObjectId},
    ports::SecretDevice,
    support::{
        oaep::write_oaep_unpadded_sha256,
        protection::ProtectedSecret,
        version::{format_semver, semver_lt},
    },
};

const SECRET_SLOT: SlotId = SlotId::Retired(RetiredSlotId::R1);
const SECRET_SLOT_CERT_OBJECT_ID: u32 = 0x005f_c10d;
const MIN_PIV_METADATA_VERSION: Version = Version {
    major: 5,
    minor: 3,
    patch: 0,
};
const YUBIKEY_MANAGEMENT_KEY_ENV: &str = "DOTFILES_YUBIKEY_PIV_MANAGEMENT_KEY_HEX";

/// 管理鍵 env を decode して `MgmKey` へ変換する。
fn management_key_from_env() -> Result<MgmKey> {
    let hex = Zeroizing::new(
        std::env::var(YUBIKEY_MANAGEMENT_KEY_ENV)
            .with_context(|| format!("{YUBIKEY_MANAGEMENT_KEY_ENV} is required"))?,
    );
    if hex.len() % 2 != 0 {
        bail!("management key hex must have even length");
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(hex.len() / 2));
    for i in (0..hex.len()).step_by(2) {
        let byte =
            u8::from_str_radix(&hex[i..i + 2], 16).context("failed to parse management key hex")?;
        bytes.push(byte);
    }
    MgmKey::from_bytes(bytes.as_slice(), None).context("failed to parse management key bytes")
}

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

    fn discover_devices(&mut self) -> Result<Vec<crate::secrets::domain::values::DeviceCandidate>> {
        let mut context = YubikeyContext::open()?;
        let mut devices = Vec::new();
        for reader in context.iter()? {
            let label = reader.name().into_owned();
            let yubikey = reader.open()?;
            devices.push(crate::secrets::domain::values::DeviceCandidate {
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
    fn serial(&self) -> u32 {
        self.yubikey.serial().0
    }

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
        let version = self.yubikey.version();
        if semver_lt(
            (version.major, version.minor, version.patch),
            (
                MIN_PIV_METADATA_VERSION.major,
                MIN_PIV_METADATA_VERSION.minor,
                MIN_PIV_METADATA_VERSION.patch,
            ),
        ) {
            bail!(
                "YubiKey PIV application version must be at least {}",
                format_semver((
                    MIN_PIV_METADATA_VERSION.major,
                    MIN_PIV_METADATA_VERSION.minor,
                    MIN_PIV_METADATA_VERSION.patch,
                ))
            );
        }
        if self.yubikey.get_pin_retries()? == 0 {
            bail!("YubiKey PIN retries are exhausted");
        }
        Ok(())
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        let key = management_key_from_env()?;
        self.yubikey.authenticate(&key)?;
        Ok(())
    }

    fn generate_key(&mut self) -> Result<()> {
        self.check_key_generation_preconditions()?;
        let key = management_key_from_env()?;
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
        let key = management_key_from_env()?;
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
        key.with_bytes(|bytes| Ok(public.encrypt(&mut OsRng, Oaep::new::<Sha256>(), bytes)?))
    }

    fn verify_pin(&mut self, pin: &SecretMaterial) -> Result<()> {
        if self.pin_verified {
            return Ok(());
        }
        pin.with_bytes(|bytes| self.yubikey.verify_pin(bytes))?;
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
        let mut decrypted = piv::decrypt_data(
            &mut self.yubikey,
            wrapped_key,
            AlgorithmId::Rsa2048,
            SECRET_SLOT,
        )?;
        let mut output = ProtectedSecret::new(Vec::new());
        write_oaep_unpadded_sha256(&decrypted, 256, &mut output)?;
        decrypted.zeroize();
        Ok(SecretMaterial::from_vec(output.into_vec()))
    }
}
