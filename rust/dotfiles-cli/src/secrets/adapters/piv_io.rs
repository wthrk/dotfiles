//! YubiKey PIV discovery/selection と実プロセス I/O を port 契約へ接続する adapter。

mod device_serial_adapter;
mod process_io_adapter;
mod report_adapter;
mod storage_adapter;
#[cfg(all(test, feature = "secrets-internal-test-stub"))]
mod selected_device {
    // `secrets-internal-test-stub` は xtask の internal test 専用経路。
    // mockito-backed test double 本体は `tests/` 配下に置き、production build には含めない。
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/secrets_internal_stub/piv_io_internal_stub.rs"
    ));
}

pub(crate) use device_serial_adapter::DeviceSelectionAdapter;
pub(crate) use process_io_adapter::ProcessIoAdapter;
pub(crate) use report_adapter::JsonReportAdapter;
pub(crate) use storage_adapter::StorageAdapter;

use crate::{
    Result,
    secrets::{
        domain::{
            material::SecretMaterial,
            piv::{PivApplicationVersion, PivObjectId, SecretStorageSpec},
        },
        support::protection::ProtectedSecret,
    },
};

#[cfg(not(all(test, feature = "secrets-internal-test-stub")))]
use crate::secrets::support::protection::{piv_pin, sealed_blob, secret_random};
#[cfg(not(all(test, feature = "secrets-internal-test-stub")))]
use anyhow::{Context, bail};
#[cfg(not(all(test, feature = "secrets-internal-test-stub")))]
use rsa::{RsaPublicKey, pkcs1::DecodeRsaPublicKey};
#[cfg(not(all(test, feature = "secrets-internal-test-stub")))]
use yubikey::{
    Context as YubikeyContext, MgmKey, PinPolicy, Serial, TouchPolicy, YubiKey,
    piv::{self, AlgorithmId, RetiredSlotId, SlotId},
};

/// `ProtectedSecret` ownership を `SecretMaterial` の opaque backend へ戻す。
///
/// caller は secret が既に protection 境界内で作られていることを保証する。この変換は
/// plaintext bytes を露出せず、後続 caller には backend contract だけを渡す。
fn material_from_protected(protected: ProtectedSecret) -> SecretMaterial {
    SecretMaterial::from_backend(protected, ProtectedSecret::len, ProtectedSecret::try_clone)
}

/// `SecretMaterial` が `ProtectedSecret` backend であることを検証して借用する。
///
/// caller は device/protection 操作へ進む前にこの関数で backend を確認する。未保護 backend は
/// device API へ渡さず、固定 error として停止する。
fn protected_from_material(secret: &SecretMaterial) -> Result<&ProtectedSecret> {
    secret
        .as_backend::<ProtectedSecret>()
        .ok_or_else(|| anyhow::anyhow!("secret material backend is not protected memory"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeviceCandidate {
    serial: u32,
    label: String,
}

/// 選択済み secret device に対する PIV storage 操作を adapter 内部の境界へ揃える。
///
/// caller は application/domain 側で決まった intent だけを渡し、この trait 実装は
/// device API と protection 操作の境界を処理する。実装は PIN や secret の平文値を
/// ログ、エラー文脈、表示出力へ露出してはならない。
trait SecretDeviceIo {
    fn key_exists(&mut self) -> Result<bool>;
    fn piv_application_version(&self) -> PivApplicationVersion;
    fn pin_retries(&mut self) -> Result<u8>;
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

/// device discovery と serial 指定 open を adapter 内部で抽象化する境界。
///
/// この trait は候補列挙と選択済み device handle の生成だけを担う。複数候補時の
/// 選択方針や use case 停止条件は caller 側が決め、実装はその判断を持ち込まない。
trait SelectedDeviceDiscoveryIo {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>>;
    fn open_device_by_serial(&mut self, serial: u32) -> Result<SelectedSecretDevice>;
}

#[cfg(not(all(test, feature = "secrets-internal-test-stub")))]
const SECRET_SLOT: SlotId = SlotId::Retired(RetiredSlotId::R1);
#[cfg(not(all(test, feature = "secrets-internal-test-stub")))]
const SECRET_SLOT_CERT_OBJECT_ID: u32 = 0x005f_c10d;

struct SelectedDeviceAdapter;

impl Default for SelectedDeviceAdapter {
    fn default() -> Self {
        Self
    }
}

struct SelectedSecretDevice {
    inner: Box<dyn SecretDeviceIo>,
}

impl SelectedSecretDevice {
    fn new(device: impl SecretDeviceIo + 'static) -> Self {
        Self {
            inner: Box::new(device),
        }
    }
}

impl SecretDeviceIo for SelectedSecretDevice {
    fn key_exists(&mut self) -> Result<bool> {
        self.inner.key_exists()
    }

    fn piv_application_version(&self) -> PivApplicationVersion {
        self.inner.piv_application_version()
    }

    fn pin_retries(&mut self) -> Result<u8> {
        self.inner.pin_retries()
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        self.inner.check_management_auth_preconditions()
    }

    fn generate_key(&mut self) -> Result<()> {
        self.inner.generate_key()
    }

    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>> {
        self.inner.read_object(object_id)
    }

    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()> {
        self.inner.write_object(object_id, value)
    }

    fn requires_pin_input(&self) -> bool {
        self.inner.requires_pin_input()
    }

    fn verify_pin(&mut self, pin: &SecretMaterial) -> Result<()> {
        self.inner.verify_pin(pin)
    }

    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        plaintext: &SecretMaterial,
    ) -> Result<Vec<u8>> {
        self.inner.seal_for_storage(storage, plaintext)
    }

    fn open_from_storage(
        &mut self,
        storage: SecretStorageSpec,
        encoded: &[u8],
    ) -> Result<SecretMaterial> {
        self.inner.open_from_storage(storage, encoded)
    }
}

#[cfg(not(all(test, feature = "secrets-internal-test-stub")))]
impl SelectedDeviceDiscoveryIo for SelectedDeviceAdapter {
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

    fn open_device_by_serial(&mut self, serial: u32) -> Result<SelectedSecretDevice> {
        Ok(SelectedSecretDevice::new(YubikeySecretDevice {
            yubikey: YubiKey::open_by_serial(Serial(serial))?,
            pin_verified: false,
        }))
    }
}

/// 実 YubiKey handle を所有し、PIV API と protected secret 操作の間を接続する。
///
/// private key は YubiKey から取り出さず、content key unwrap は PIN 検証済み状態で
/// hardware operation として実行する。caller は secret 読み出し前に `verify_pin` を
/// 通して PIN 検証状態を確立する責任を持つ。
#[cfg(not(all(test, feature = "secrets-internal-test-stub")))]
struct YubikeySecretDevice {
    yubikey: YubiKey,
    pin_verified: bool,
}

#[cfg(not(all(test, feature = "secrets-internal-test-stub")))]
impl YubikeySecretDevice {
    fn default_management_key(&self) -> Result<MgmKey> {
        MgmKey::get_default(&self.yubikey).context("failed to load default YubiKey management key")
    }

    fn piv_application_version(&self) -> PivApplicationVersion {
        let version = self.yubikey.version();
        PivApplicationVersion {
            major: version.major,
            minor: version.minor,
            patch: version.patch,
        }
    }

    fn wrap_content_key(
        &mut self,
        key: &crate::secrets::support::protection::ProtectedSecret,
    ) -> Result<Vec<u8>> {
        let metadata = piv::metadata(&mut self.yubikey, SECRET_SLOT)?;
        let public = metadata
            .public
            .context("YubiKey secret storage key has no public key metadata")?;
        let public = RsaPublicKey::from_pkcs1_der(public.subject_public_key.raw_bytes())
            .context("failed to parse YubiKey secret storage public key")?;
        secret_random::rsa_oaep_encrypt(&public, key)
    }

    fn unwrap_content_key(
        &mut self,
        wrapped_key: &[u8],
    ) -> Result<crate::secrets::support::protection::ProtectedSecret> {
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

#[cfg(not(all(test, feature = "secrets-internal-test-stub")))]
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

    fn piv_application_version(&self) -> PivApplicationVersion {
        self.piv_application_version()
    }

    fn pin_retries(&mut self) -> Result<u8> {
        self.yubikey.get_pin_retries().map_err(anyhow::Error::new)
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        let key = self.default_management_key()?;
        self.yubikey.authenticate(&key)?;
        Ok(())
    }

    fn generate_key(&mut self) -> Result<()> {
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

    fn requires_pin_input(&self) -> bool {
        !self.pin_verified
    }

    fn verify_pin(&mut self, pin: &SecretMaterial) -> Result<()> {
        if self.pin_verified {
            return Ok(());
        }
        piv_pin::verify_pin(
            protected_from_material(pin)?,
            &mut YubikeyPinVerifier(&mut self.yubikey),
        )?;
        self.pin_verified = true;
        Ok(())
    }

    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        plaintext: &SecretMaterial,
    ) -> Result<Vec<u8>> {
        sealed_blob::seal_with_key_wrap(
            sealed_blob::SealWithKeyWrapRequest {
                payload_id: storage.secret_id,
                plaintext: protected_from_material(plaintext)?,
                aad: &storage.additional_data,
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
        )
        .map(material_from_protected)
    }
}

#[cfg(not(all(test, feature = "secrets-internal-test-stub")))]
struct YubikeyPinVerifier<'a>(&'a mut YubiKey);

#[cfg(not(all(test, feature = "secrets-internal-test-stub")))]
impl piv_pin::PivPinVerifier for YubikeyPinVerifier<'_> {
    fn verify(&mut self, bytes: &[u8]) -> Result<()> {
        self.0.verify_pin(bytes).map_err(anyhow::Error::new)
    }
}
