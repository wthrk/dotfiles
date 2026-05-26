use yubikey::{Context as YubikeyContext, Serial, YubiKey};

use crate::{
    Result,
    secrets::domain::{
        BootstrapSecretDocument, CONTENT_KEY_LEN, NONCE_LEN, PivObjectId, SecretBlob,
        SecretManifest, SecretName, StorageObjectIds, aes_256_gcm_from_key,
        decode_initialized_manifest, decrypt_detached, encode_manifest, encrypt_detached,
        ensure_secret_value_non_empty,
    },
    secrets::ports::RandomBytesPort,
};
use anyhow::{Context, anyhow, bail};
use zeroize::Zeroizing;

use crate::secrets::{
    adapters::yubikey::YubikeySecretDevice,
    ports::{DeviceSelectionPort, SecretDevice},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredDevice {
    pub serial: u32,
    pub label: String,
}

pub(crate) struct RealDeviceAdapter;

impl RealDeviceAdapter {
    /// production で実機 YubiKey へ接続する concrete device adapter を返す。
    pub(crate) fn production() -> Self {
        Self
    }
}

impl DeviceSelectionPort for RealDeviceAdapter {
    type Device = YubikeySecretDevice;
    type DeviceCandidate = DiscoveredDevice;

    fn discover_devices(&mut self) -> Result<Vec<Self::DeviceCandidate>> {
        let mut context = YubikeyContext::open()?;
        let mut devices = Vec::new();
        for reader in context.iter()? {
            let label = reader.name().into_owned();
            let yubikey = reader.open()?;
            devices.push(DiscoveredDevice {
                serial: yubikey.serial().0,
                label,
            });
        }
        Ok(devices)
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<Self::Device> {
        Ok(YubikeySecretDevice {
            yubikey: YubiKey::open_by_serial(Serial(serial))?,
            pin_verified: false,
        })
    }
}

pub(crate) trait SecretDeviceExt: SecretDevice {
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
        crate::secrets::domain::ensure_storage_setup_allowed(
            key_exists,
            manifest_bytes.as_deref(),
            &occupied_object_ids,
        )?;

        self.generate_key()?;
        let mut manifest = encode_manifest(&SecretManifest::expected())?;
        self.write_object(PivObjectId::MANIFEST, &mut manifest)
    }

    fn store_secret(
        &mut self,
        random: &impl RandomBytesPort,
        name: SecretName,
        secret: &[u8],
        force: bool,
    ) -> Result<()> {
        ensure_secret_value_non_empty(name, secret)?;
        self.ensure_storage_initialized()?;
        self.check_management_auth_preconditions()?;
        if self.read_object(name.object_id())?.is_some() && !force {
            bail!("{} already exists; pass --force to replace it", name);
        }

        let mut content_key = Zeroizing::new([0u8; CONTENT_KEY_LEN]);
        random.fill_random_bytes(&mut *content_key)?;
        let mut nonce = [0u8; NONCE_LEN];
        random.fill_random_bytes(&mut nonce)?;
        let cipher = aes_256_gcm_from_key(content_key.as_ref())?;

        let mut ciphertext = Zeroizing::new(secret.to_vec());
        let tag = encrypt_detached(
            &cipher,
            &nonce,
            &name.additional_data(self.serial()),
            ciphertext.as_mut_slice(),
        )?;
        let wrapped_key = self.wrap_key(content_key.as_ref())?;
        let blob = SecretBlob {
            name,
            nonce,
            wrapped_key,
            ciphertext: ciphertext.to_vec(),
            tag,
        };

        let mut encoded = blob.encode()?;
        self.write_object(name.object_id(), &mut encoded)
    }

    fn load_secret(&mut self, name: SecretName) -> Result<Zeroizing<Vec<u8>>> {
        self.ensure_storage_initialized()?;
        let encoded = self
            .read_object(name.object_id())?
            .with_context(|| format!("{} is not stored on this YubiKey", name))?;
        let blob =
            SecretBlob::decode(&encoded).with_context(|| format!("failed to decode {}", name))?;
        if blob.name != name {
            bail!("YubiKey secret blob name does not match requested {}", name);
        }

        let content_key = self.unwrap_key(&blob.wrapped_key)?;
        if content_key.len() != CONTENT_KEY_LEN {
            bail!("unwrapped YubiKey content key has invalid length");
        }

        let cipher = aes_256_gcm_from_key(&content_key)?;
        let mut secret = Zeroizing::new(blob.ciphertext.clone());
        decrypt_detached(
            &cipher,
            &blob.nonce,
            &blob.name.additional_data(self.serial()),
            secret.as_mut_slice(),
            &blob.tag,
        )
        .map_err(|_| anyhow!("failed to decrypt {}", blob.name))?;
        Ok(secret)
    }

    fn store_bootstrap_secret_document(
        &mut self,
        random: &impl RandomBytesPort,
        document: &BootstrapSecretDocument,
    ) -> Result<()> {
        self.store_secret(
            random,
            SecretName::BwEmail,
            document.bw_email.as_bytes(),
            false,
        )?;
        self.store_secret(
            random,
            SecretName::BwPassword,
            document.bw_password.as_bytes(),
            false,
        )?;
        self.store_secret(
            random,
            SecretName::BwsAccessToken,
            document.bws_access_token.as_bytes(),
            false,
        )
    }

    fn verify_required_secrets(&mut self) -> Result<()> {
        for name in SecretName::iter() {
            let secret = self.load_secret(name)?;
            ensure_secret_value_non_empty(name, secret.as_ref())?;
        }
        Ok(())
    }

    fn ensure_storage_initialized(&mut self) -> Result<SecretManifest> {
        let manifest_bytes = self.read_object(PivObjectId::MANIFEST)?;
        decode_initialized_manifest(manifest_bytes.as_deref())
    }
}

impl SecretDeviceExt for YubikeySecretDevice {}

#[cfg(not(feature = "secrets-test-stub"))]
pub(crate) type SelectedDeviceAdapter = RealDeviceAdapter;
#[cfg(feature = "secrets-test-stub")]
pub(crate) use super::device_test_stub::TestStubDeviceAdapter as SelectedDeviceAdapter;
