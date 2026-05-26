//! 実機 YubiKey PIV セッションを `SecretDevice` port へ接続する adapter。

use aes_gcm::{Aes256Gcm, KeyInit, aead::AeadInPlace};
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
    domain::{
        blob::{CONTENT_KEY_LEN, NONCE_LEN, SecretBlob},
        manifest::SecretManifest,
        material::SecretMaterial,
        piv::{PivObjectId, SecretName, StorageObjectIds},
    },
    ports::{RandomBytesPort, SecretDevice},
    support::{
        oaep::write_oaep_unpadded_sha256,
        protection::ProtectedSecret,
        version::{format_semver, semver_lt},
    },
};

const SECRET_SLOT: SlotId = SlotId::Retired(RetiredSlotId::R1);
const SECRET_SLOT_CERT_OBJECT_ID: u32 = 0x005f_c10d;
const AEAD_NONCE_LEN: usize = 12;
const AEAD_TAG_LEN: usize = 16;
const MIN_PIV_METADATA_VERSION: Version = Version {
    major: 5,
    minor: 3,
    patch: 0,
};
const YUBIKEY_MANAGEMENT_KEY_ENV: &str = "DOTFILES_YUBIKEY_PIV_MANAGEMENT_KEY_HEX";

/// 管理鍵 env を decode して `MgmKey` へ変換する。
///
/// 実機 adapter だけが env 依存を吸収し、domain/application へ管理鍵形式を漏らさない。
/// 欠落・hex 破損・長さ不正はすべて失敗として扱う。
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

/// content key bytes を AES-256-GCM 実装へ変換する。
///
/// 鍵長検証を adapter 境界で失敗させ、domain/application へ暗号ライブラリ固有の
/// 初期化制約を漏らさないためにこの helper で一元化する。
/// この関数は「有効鍵長で初期化できた cipher だけを下流へ渡す」安全境界であり、
/// 以降の encrypt/decrypt 経路では鍵長エラー分岐を持ち込まない。
fn aes_256_gcm_from_key(key: &[u8]) -> Result<Aes256Gcm> {
    Aes256Gcm::new_from_slice(key).context("invalid AES-256-GCM key length")
}

/// AES-GCM で `buffer` を in-place 暗号化し detached tag を返す。
///
/// Why: 暗号化処理を adapter 境界に集約し、domain/application へ
/// AES 実装依存の nonce/tag 取り扱いを漏らさないため。
/// Caller responsibility: `cipher` と `nonce` は同じ content key 文脈で
/// 一貫して与え、`buffer` は secret payload として上書きされる前提で渡すこと。
/// 停止条件: nonce 長が `AEAD_NONCE_LEN` でない場合、または暗号化/タグ変換に失敗した場合は
/// 即時にエラーを返して処理を継続しない。
fn encrypt_detached(
    cipher: &Aes256Gcm,
    nonce: &[u8],
    additional_data: &[u8],
    buffer: &mut [u8],
) -> Result<[u8; AEAD_TAG_LEN]> {
    if nonce.len() != AEAD_NONCE_LEN {
        bail!("invalid AES-256-GCM nonce length");
    }
    let tag = cipher
        .encrypt_in_place_detached(aes_gcm::Nonce::from_slice(nonce), additional_data, buffer)
        .map_err(|error| anyhow::anyhow!("AES-GCM encrypt failed: {error:?}"))
        .context("failed to encrypt protected payload")?;
    tag.as_slice()
        .try_into()
        .map_err(anyhow::Error::new)
        .context("failed to encode AES-GCM tag")
}

/// AES-GCM の detached tag を使って `buffer` を in-place 復号する。
///
/// Why: 復号と完全性検証を単一境界で実施し、検証失敗時の扱いを統一するため。
/// Caller responsibility: `nonce`・`additional_data`・`tag` は暗号化時と同一値を渡し、
/// `buffer` は検証失敗時に平文として利用しないこと。
/// 停止条件: nonce/tag 長不一致、または認証付き復号の検証失敗時は
/// 即時エラーで停止し、復号結果を下流へ渡さない。
fn decrypt_detached(
    cipher: &Aes256Gcm,
    nonce: &[u8],
    additional_data: &[u8],
    buffer: &mut [u8],
    tag: &[u8],
) -> Result<()> {
    if nonce.len() != AEAD_NONCE_LEN {
        bail!("invalid AES-256-GCM nonce length");
    }
    if tag.len() != AEAD_TAG_LEN {
        bail!("invalid AES-GCM tag length");
    }
    cipher
        .decrypt_in_place_detached(
            aes_gcm::Nonce::from_slice(nonce),
            additional_data,
            buffer,
            aes_gcm::Tag::from_slice(tag),
        )
        .map_err(|error| anyhow::anyhow!("AES-GCM decrypt failed: {error:?}"))
        .context("failed to decrypt protected payload")
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

    fn setup_storage(&mut self) -> Result<()> {
        self.check_key_generation_preconditions()?;
        self.check_management_auth_preconditions()?;
        let key_exists = self.key_exists()?;
        let manifest_bytes = self.read_object(PivObjectId::MANIFEST)?;
        let mut occupied_object_ids = Vec::new();
        for object_id in StorageObjectIds::iter() {
            if self.read_object(object_id)?.is_some() {
                occupied_object_ids.push(object_id);
            }
        }
        SecretManifest::ensure_setup_allowed(
            key_exists,
            manifest_bytes.as_deref(),
            &occupied_object_ids,
        )?;
        self.generate_key()?;
        let mut manifest = SecretManifest::expected().encode()?;
        self.write_object(PivObjectId::MANIFEST, &mut manifest)
    }

    fn store_secret(
        &mut self,
        random: &impl RandomBytesPort,
        name: SecretName,
        secret: &SecretMaterial,
        force: bool,
    ) -> Result<()> {
        secret.with_bytes(|bytes| name.ensure_value_non_empty(bytes))?;
        SecretManifest::decode_initialized(self.read_object(PivObjectId::MANIFEST)?.as_deref())?;
        self.check_management_auth_preconditions()?;
        if self.read_object(name.object_id())?.is_some() && !force {
            bail!("{} already exists; pass --force to replace it", name);
        }
        let mut content_key = ProtectedSecret::new(vec![0u8; CONTENT_KEY_LEN]);
        content_key.with_secret_mut(|value| random.fill_random_bytes(value))?;
        let content_key = SecretMaterial::from_vec(content_key.into_vec());
        let mut nonce = [0u8; NONCE_LEN];
        random.fill_random_bytes(&mut nonce)?;
        let cipher = content_key.with_bytes(aes_256_gcm_from_key)?;
        let mut ciphertext = secret.with_bytes(|bytes| ProtectedSecret::new(bytes.to_vec()));
        let tag = ciphertext.with_secret_mut(|ciphertext_bytes| {
            encrypt_detached(
                &cipher,
                &nonce,
                &name.additional_data(self.serial()),
                ciphertext_bytes,
            )
        })?;
        let wrapped_key = self.wrap_key(&content_key)?;
        let blob = SecretBlob {
            name,
            nonce,
            wrapped_key,
            ciphertext: ciphertext.into_vec(),
            tag,
        };
        let mut encoded = blob.encode()?;
        self.write_object(name.object_id(), &mut encoded)
    }

    fn load_secret(&mut self, name: SecretName) -> Result<SecretMaterial> {
        SecretManifest::decode_initialized(self.read_object(PivObjectId::MANIFEST)?.as_deref())?;
        let encoded = self
            .read_object(name.object_id())?
            .with_context(|| format!("{} is not stored on this YubiKey", name))?;
        let blob =
            SecretBlob::decode(&encoded).with_context(|| format!("failed to decode {}", name))?;
        if blob.name != name {
            bail!("YubiKey secret blob name does not match requested {}", name);
        }
        let SecretBlob {
            name: blob_name,
            nonce,
            wrapped_key,
            ciphertext,
            tag,
        } = blob;
        let content_key = self.unwrap_key(&wrapped_key)?;
        if content_key.len() != CONTENT_KEY_LEN {
            bail!("unwrapped YubiKey content key has invalid length");
        }
        let cipher = content_key.with_bytes(aes_256_gcm_from_key)?;
        let mut secret = ProtectedSecret::new(ciphertext);
        secret
            .with_secret_mut(|secret_bytes| {
                decrypt_detached(
                    &cipher,
                    &nonce,
                    &blob_name.additional_data(self.serial()),
                    secret_bytes,
                    &tag,
                )
            })
            .map_err(|_| anyhow::anyhow!("failed to decrypt {}", blob_name))?;
        Ok(SecretMaterial::from_vec(secret.into_vec()))
    }
    fn verify_required_secrets(&mut self) -> Result<()> {
        for name in SecretName::iter() {
            let secret = self.load_secret(name)?;
            secret.with_bytes(|bytes| name.ensure_value_non_empty(bytes))?;
        }
        Ok(())
    }
}
