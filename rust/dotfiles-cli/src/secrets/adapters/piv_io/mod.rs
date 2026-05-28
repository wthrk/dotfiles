//! YubiKey PIV discovery/selection と実プロセス I/O を port 契約へ接続する adapter。

mod device_selection;
mod process_io_adapter;
mod report_adapter;
mod storage_adapter;
pub(crate) use device_selection::DeviceSelectionAdapter;
pub(crate) use process_io_adapter::ProcessIoAdapter;
pub(crate) use report_adapter::JsonReportAdapter;
pub(crate) use storage_adapter::StorageAdapter;

#[cfg(not(feature = "secrets-internal-test-stub"))]
use anyhow::{bail, Context};
#[cfg(not(feature = "secrets-internal-test-stub"))]
use rsa::{pkcs1::DecodeRsaPublicKey, RsaPublicKey};
#[cfg(not(feature = "secrets-internal-test-stub"))]
use yubikey::{
    piv::{self, AlgorithmId, RetiredSlotId, SlotId},
    Context as YubikeyContext, MgmKey, PinPolicy, Serial, TouchPolicy, YubiKey,
};

use crate::{
    secrets::{
        domain::{
            material::SecretMaterial,
            piv::{PivApplicationVersion, PivObjectId, SecretStorageSpec},
        },
        support::protection::{secret_consumer, ProtectedSecret},
    },
    Result,
};

#[cfg(feature = "secrets-internal-test-stub")]
mod internal_stub {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/secrets_internal_stub/piv_io_internal_stub.rs"
    ));
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
use crate::secrets::support::protection::{sealed_blob, secret_random};

#[cfg(not(feature = "secrets-internal-test-stub"))]
const SECRET_SLOT: SlotId = SlotId::Retired(RetiredSlotId::R1);
#[cfg(not(feature = "secrets-internal-test-stub"))]
const SECRET_SLOT_CERT_OBJECT_ID: u32 = 0x005f_c10d;

fn material_from_protected(protected: ProtectedSecret) -> SecretMaterial {
    SecretMaterial::from_backend(protected, ProtectedSecret::len, ProtectedSecret::try_clone)
}

/// `SecretMaterial` の backend が `ProtectedSecret` であることを確認して参照を返す。
///
/// adapter が secret backend へ直接触る境界をこの関数に限定し、backend 不一致時は即座に失敗する。
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

#[cfg(not(feature = "secrets-internal-test-stub"))]
/// 実デバイス列挙と serial 指定オープンを隠蔽し、選択経路の外部依存を隔離する内部境界。
trait RealDeviceIo {
    /// 接続済みデバイスを列挙し、対話選択に必要な表示情報を返す。
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>>;
    /// 指定 serial のデバイスを開き、secret storage 操作用の実装へ接続する。
    fn open_device_by_serial(&mut self, serial: u32) -> Result<YubikeySecretDevice>;
}

/// Secret storage 操作を YubiKey 実装へ橋渡しする内部境界。
///
/// use case 手順は保持せず、object I/O と暗号化 payload の read/write のみを扱う。
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

/// device selection adapter が利用する discovery 境界。
///
/// 実ビルドと internal test stub の分岐をこの trait 実装に閉じ込める。
trait SelectedDeviceDiscoveryIo {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>>;
    fn open_device_by_serial(&mut self, serial: u32) -> Result<SelectedSecretDevice>;
}

/// `SelectedDeviceDiscoveryIo` の default 実装ルートを提供する境界型。
struct SelectedDeviceAdapter;

const ADAPTER_ROUTE_AUDIT_PREFIX: &str = "DOTFILES_SECRETS_DEVICE_ADAPTER_ROUTE";

#[cfg(not(feature = "secrets-internal-test-stub"))]
fn selected_device_route_label() -> &'static str {
    "real"
}

#[cfg(feature = "secrets-internal-test-stub")]
fn selected_device_route_label() -> &'static str {
    "stub"
}

impl Default for SelectedDeviceAdapter {
    fn default() -> Self {
        eprintln!(
            "{ADAPTER_ROUTE_AUDIT_PREFIX}={}",
            selected_device_route_label()
        );
        Self
    }
}

struct SelectedSecretDevice {
    inner: Box<dyn SecretDeviceIo>,
}

impl SelectedSecretDevice {
    /// `SecretDeviceIo` 実装を trait object として保持し、呼び出し側から実装差分を隠蔽する。
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

#[cfg(not(feature = "secrets-internal-test-stub"))]
impl SelectedDeviceDiscoveryIo for SelectedDeviceAdapter {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>> {
        RealDeviceIo::discover_devices(&mut RealDeviceAdapter)
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<SelectedSecretDevice> {
        RealDeviceIo::open_device_by_serial(&mut RealDeviceAdapter, serial)
            .map(SelectedSecretDevice::new)
    }
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
/// `SelectedDeviceDiscoveryIo` の実環境ルート実装。
///
/// 列挙と open 以外の手順制御は持たず、device 選択の外部 I/O 翻訳に限定する。
struct YubikeySecretDevice {
    yubikey: YubiKey,
    pin_verified: bool,
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
impl YubikeySecretDevice {
    /// serial を指定して YubiKey デバイスを開き、secret-device 境界へ接続する。
    fn open_by_serial(serial: u32) -> Result<Self> {
        Ok(Self {
            yubikey: YubiKey::open_by_serial(Serial(serial))?,
            pin_verified: false,
        })
    }

    fn default_management_key(&self) -> Result<MgmKey> {
        MgmKey::get_default(&self.yubikey).context("failed to load default YubiKey management key")
    }

    /// PIV application version を domain 境界型へ変換する。
    fn piv_application_version(&self) -> PivApplicationVersion {
        let version = self.yubikey.version();
        PivApplicationVersion {
            major: version.major,
            minor: version.minor,
            patch: version.patch,
        }
    }

    /// content key を YubiKey の公開鍵で wrap し、保存用 payload へ渡す。
    fn wrap_content_key(&mut self, key: &ProtectedSecret) -> Result<Vec<u8>> {
        let metadata = piv::metadata(&mut self.yubikey, SECRET_SLOT)?;
        let public = metadata
            .public
            .context("YubiKey secret storage key has no public key metadata")?;
        let public = RsaPublicKey::from_pkcs1_der(public.subject_public_key.raw_bytes())
            .context("failed to parse YubiKey secret storage public key")?;
        secret_random::rsa_oaep_encrypt(&public, key)
    }

    /// wrap された content key を unwrap する。
    ///
    /// PIN 未検証状態での復号を禁止し、read 経路の前提条件を adapter 境界で強制する。
    fn unwrap_content_key(&mut self, wrapped_key: &[u8]) -> Result<ProtectedSecret> {
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

#[cfg(not(feature = "secrets-internal-test-stub"))]
/// `RealDeviceIo` の実装型。YubiKey 列挙と serial オープンの外部 API 変換のみを担当する。
struct RealDeviceAdapter;

#[cfg(not(feature = "secrets-internal-test-stub"))]
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

#[cfg(not(feature = "secrets-internal-test-stub"))]
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
    fn verify_pin(&mut self, pin: &SecretMaterial) -> Result<()> {
        if self.pin_verified {
            return Ok(());
        }
        secret_consumer::consume(
            protected_from_material(pin)?,
            &mut YubikeyPinVerifier(&mut self.yubikey),
        )?;
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

#[cfg(not(feature = "secrets-internal-test-stub"))]
/// PIN 検証に必要な byte 消費処理を `secret_consumer` 契約へ接続するアダプター。
struct YubikeyPinVerifier<'a>(&'a mut YubiKey);

#[cfg(not(feature = "secrets-internal-test-stub"))]
impl secret_consumer::SecretConsumer for YubikeyPinVerifier<'_> {
    fn consume(&mut self, bytes: &[u8]) -> Result<()> {
        self.0.verify_pin(bytes).map_err(anyhow::Error::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_device_adapter_route_is_compile_time_selected() {
        #[cfg(not(feature = "secrets-internal-test-stub"))]
        assert_eq!(selected_device_route_label(), "real");
        #[cfg(feature = "secrets-internal-test-stub")]
        assert_eq!(selected_device_route_label(), "stub");
    }
}
