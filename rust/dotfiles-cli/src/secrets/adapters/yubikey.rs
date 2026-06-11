//! YubiKey PIV discovery/selection と secret storage を YubiKey port 契約へ接続する adapter。
//!
//! process I/O と report 出力は `adapters::io` 側へ分離し、この module は YubiKey backend の
//! discovery、PIN 要否、PIV object storage 翻訳だけを扱う。

mod device_serial_adapter;
mod storage_adapter;
// `secrets-internal-test-stub` feature 専用の adapter backend stub。
//
// この module は production build には含めず、runtime 分岐ではなく compile-time feature
// selection で実 YubiKey backend と差し替える。integration test は adapter stub module を
// import せず、feature 有効でビルドされた同じ `dotfiles` binary を実行し、
// YubiKey port 専用の初期条件 spec JSON と最終状態観測 JSON だけを外部観測面として扱う。
#[cfg(feature = "secrets-internal-test-stub")]
mod selected_device;

use crate::{
    Result,
    secrets::{
        domain::{
            gpg_backup::{ConnectedYubiKey, EnvelopeRecipient},
            piv::{PivApplicationVersion, PivObjectId, SecretStorageSpec},
            storage::{
                SecretStorageReadInspection, SecretStorageReadIntent, SecretStorageSetupInspection,
                SecretStorageSetupIntent, SecretStorageSetupProbe, SecretStorageWriteInspection,
                SecretStorageWriteIntent,
            },
        },
        ports::yubikey::{
            DevicePinPolicyPort, DeviceSerialPort, GpgRecipientPort, SecretStoragePort,
        },
        support::protection::{ProtectedSecret, SecretSession},
    },
};

/// YubiKey discovery と PIN 要否判定を port 契約へ翻訳する adapter。
#[derive(Default)]
pub(in crate::secrets) struct DeviceSelectionAdapter(device_serial_adapter::DeviceSelectionAdapter);

impl DeviceSerialPort for DeviceSelectionAdapter {
    fn resolve_device_serial(&mut self) -> Result<u32> {
        self.0.resolve_device_serial()
    }
}

impl DevicePinPolicyPort for DeviceSelectionAdapter {
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool> {
        self.0.device_requires_pin(serial)
    }
}

/// YubiKey storage I/O を port 契約へ翻訳する adapter。
#[derive(Default)]
pub(in crate::secrets) struct StorageAdapter(storage_adapter::StorageAdapter);

impl SecretStoragePort for StorageAdapter {
    fn inspect_secret_storage_setup(
        &mut self,
        serial: u32,
        probe: &SecretStorageSetupProbe,
    ) -> Result<SecretStorageSetupInspection> {
        self.0.inspect_secret_storage_setup(serial, probe)
    }

    fn initialize_secret_storage(
        &mut self,
        serial: u32,
        intent: SecretStorageSetupIntent,
    ) -> Result<()> {
        self.0.initialize_secret_storage(serial, intent)
    }

    fn finalize_secret_storage_setup(
        &mut self,
        serial: u32,
        intent: SecretStorageSetupIntent,
    ) -> Result<()> {
        self.0.finalize_secret_storage_setup(serial, intent)
    }

    fn inspect_secret_storage_write(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageWriteInspection> {
        self.0.inspect_secret_storage_write(serial, storage)
    }

    fn store_secret(
        &mut self,
        serial: u32,
        intent: SecretStorageWriteIntent,
        secret: &ProtectedSecret,
    ) -> Result<()> {
        self.0.store_secret(serial, intent, secret)
    }

    fn inspect_secret_storage_read(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageReadInspection> {
        self.0.inspect_secret_storage_read(serial, storage)
    }

    fn load_secret(
        &mut self,
        serial: u32,
        intent: &SecretStorageReadIntent,
        pin: Option<&ProtectedSecret>,
    ) -> Result<ProtectedSecret> {
        self.0.load_secret(serial, intent, pin)
    }
}

/// `gpg-secret-key-backup` recipient 運用（PIV slot 82 公開鍵）を port 契約へ翻訳する adapter。
#[derive(Default)]
pub(in crate::secrets) struct GpgRecipientAdapter {
    device: SelectedDeviceAdapter,
}

impl GpgRecipientAdapter {
    fn open_device_by_serial(&mut self, serial: u32) -> Result<SelectedSecretDevice> {
        SelectedDeviceDiscoveryIo::open_device_by_serial(&mut self.device, serial)
    }
}

impl GpgRecipientPort for GpgRecipientAdapter {
    fn resolve_connected_recipient(&mut self, serial: u32) -> Result<ConnectedYubiKey> {
        let mut device = self.open_device_by_serial(serial)?;
        let fingerprint = device.recipient_public_key_fingerprint()?;
        ConnectedYubiKey::new(&fingerprint)
    }

    fn unwrap_dek(
        &mut self,
        serial: u32,
        recipient: &EnvelopeRecipient,
        pin: Option<&ProtectedSecret>,
    ) -> Result<ProtectedSecret> {
        let _session = SecretSession::start()?;
        let mut device = self.open_device_by_serial(serial)?;
        if device.requires_pin_input() {
            let Some(pin) = pin else {
                anyhow::bail!("PIN is required to unwrap the gpg backup DEK");
            };
            device.verify_pin(pin)?;
        }
        device.unwrap_dek(recipient.wrapped_dek())
    }
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
use crate::secrets::support::protection::{piv_pin, sealed_blob, secret_random};
#[cfg(not(feature = "secrets-internal-test-stub"))]
use anyhow::{Context, bail};
#[cfg(not(feature = "secrets-internal-test-stub"))]
use rsa::{RsaPublicKey, pkcs1::DecodeRsaPublicKey, pkcs8::EncodePublicKey};
#[cfg(not(feature = "secrets-internal-test-stub"))]
use sha2::{Digest, Sha256};
#[cfg(not(feature = "secrets-internal-test-stub"))]
use yubikey::{
    Context as YubikeyContext, MgmKey, PinPolicy, Serial, TouchPolicy, YubiKey,
    piv::{self, AlgorithmId, RetiredSlotId, SlotId},
};

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
    fn verify_pin(&mut self, pin: &ProtectedSecret) -> Result<()>;
    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        plaintext: &ProtectedSecret,
    ) -> Result<Vec<u8>>;
    fn open_from_storage(
        &mut self,
        storage: SecretStorageSpec,
        encoded: &[u8],
    ) -> Result<ProtectedSecret>;
    /// PIV slot `82` 公開鍵の DER-encoded SubjectPublicKeyInfo を SHA-256 した lowercase hex（64 文字）を返す。
    ///
    /// `gpg-secret-key-backup` recipient の `public_key_fingerprint` に対応する。公開鍵は秘密情報ではない。
    fn recipient_public_key_fingerprint(&mut self) -> Result<String>;
    /// PIV slot `82` 秘密鍵で wrapped DEK を device 内 RSA decrypt + OAEP unwrap して DEK を復元する。
    fn unwrap_dek(&mut self, wrapped_dek: &[u8]) -> Result<ProtectedSecret>;
}

/// device discovery と serial 指定 open を adapter 内部で抽象化する境界。
///
/// この trait は候補列挙と、解決済み serial に対する device handle の生成だけを担う。
/// 複数候補時に対象を受け付けない停止条件は device serial adapter 側で完了する。
trait SelectedDeviceDiscoveryIo {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>>;
    fn open_device_by_serial(&mut self, serial: u32) -> Result<SelectedSecretDevice>;
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
const SECRET_SLOT: SlotId = SlotId::Retired(RetiredSlotId::R1);
#[cfg(not(feature = "secrets-internal-test-stub"))]
const SECRET_SLOT_CERT_OBJECT_ID: u32 = 0x005f_c10d;

/// 実行時の YubiKey discovery backend を選択する adapter 内部境界。
///
/// production build では実 YubiKey API に接続し、internal test feature では同じ module tree の
/// `selected_device` backend を compile 時の feature gate で選ぶ。caller は discovery/open の結果だけを
/// 使い、backend の実体へ依存しない。
struct SelectedDeviceAdapter;

impl Default for SelectedDeviceAdapter {
    fn default() -> Self {
        Self
    }
}

/// 選択済み device handle を type-erased PIV I/O 境界として保持する adapter 内部型。
///
/// caller は `SecretDeviceIo` 契約だけを通じて操作し、実 YubiKey handle や test double の型を
/// storage/device selection adapter の外へ漏らさない。
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

    fn verify_pin(&mut self, pin: &ProtectedSecret) -> Result<()> {
        self.inner.verify_pin(pin)
    }

    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        plaintext: &ProtectedSecret,
    ) -> Result<Vec<u8>> {
        self.inner.seal_for_storage(storage, plaintext)
    }

    fn open_from_storage(
        &mut self,
        storage: SecretStorageSpec,
        encoded: &[u8],
    ) -> Result<ProtectedSecret> {
        self.inner.open_from_storage(storage, encoded)
    }

    fn recipient_public_key_fingerprint(&mut self) -> Result<String> {
        self.inner.recipient_public_key_fingerprint()
    }

    fn unwrap_dek(&mut self, wrapped_dek: &[u8]) -> Result<ProtectedSecret> {
        self.inner.unwrap_dek(wrapped_dek)
    }
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
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
#[cfg(not(feature = "secrets-internal-test-stub"))]
struct YubikeySecretDevice {
    yubikey: YubiKey,
    pin_verified: bool,
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
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

    fn wrap_content_key(&mut self, key: &ProtectedSecret) -> Result<Vec<u8>> {
        let metadata = piv::metadata(&mut self.yubikey, SECRET_SLOT)?;
        let public = metadata
            .public
            .context("YubiKey secret storage key has no public key metadata")?;
        let public = RsaPublicKey::from_pkcs1_der(public.subject_public_key.raw_bytes())
            .context("failed to parse YubiKey secret storage public key")?;
        secret_random::rsa_oaep_encrypt(&public, key)
    }

    fn unwrap_content_key(&mut self, wrapped_key: &[u8]) -> Result<ProtectedSecret> {
        if !self.pin_verified {
            bail!("YubiKey PIN must be verified before reading stored secrets");
        }
        sealed_blob::unwrap_content_key_from_decrypt(
            || {
                piv::decrypt_data(
                    &mut self.yubikey,
                    wrapped_key,
                    AlgorithmId::Rsa2048,
                    SECRET_SLOT,
                )
                .context("failed to decrypt wrapped content key with YubiKey PIV")
            },
            256,
        )
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
        self.yubikey
            .get_pin_retries()
            .context("failed to query YubiKey PIN retry counter")
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

    fn verify_pin(&mut self, pin: &ProtectedSecret) -> Result<()> {
        if self.pin_verified {
            return Ok(());
        }
        piv_pin::verify_pin(pin, &mut YubikeyPinVerifier(&mut self.yubikey))?;
        self.pin_verified = true;
        Ok(())
    }

    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        plaintext: &ProtectedSecret,
    ) -> Result<Vec<u8>> {
        sealed_blob::seal_material_with_key_wrap(
            storage.secret_id,
            plaintext,
            &storage.additional_data,
            |content_key| self.wrap_content_key(content_key),
        )
    }

    fn open_from_storage(
        &mut self,
        storage: SecretStorageSpec,
        encoded: &[u8],
    ) -> Result<ProtectedSecret> {
        sealed_blob::open_material_with_key_unwrap(
            encoded,
            storage.secret_id,
            |wrapped_key| self.unwrap_content_key(wrapped_key),
            &storage.additional_data,
        )
    }

    fn recipient_public_key_fingerprint(&mut self) -> Result<String> {
        let metadata = piv::metadata(&mut self.yubikey, SECRET_SLOT)?;
        let public = metadata
            .public
            .context("YubiKey secret storage key has no public key metadata")?;
        // 設計: recipient `public_key_fingerprint` は DER-encoded SubjectPublicKeyInfo の SHA-256。
        // slot 82 の RSA 公開鍵を SubjectPublicKeyInfo DER として再エンコードし、その digest を取る。
        let rsa_public = RsaPublicKey::from_pkcs1_der(public.subject_public_key.raw_bytes())
            .context("failed to parse YubiKey slot 82 public key")?;
        let der = rsa_public
            .to_public_key_der()
            .context("failed to DER-encode YubiKey slot 82 public key")?;
        Ok(sha256_lowercase_hex(der.as_bytes()))
    }

    fn unwrap_dek(&mut self, wrapped_dek: &[u8]) -> Result<ProtectedSecret> {
        self.unwrap_content_key(wrapped_dek)
    }
}

/// YubiKey crate の PIN 検証 API を protection 境界の verifier contract へ接続する。
///
/// PIN bytes は `piv_pin::verify_pin` の借用中だけこの adapter へ渡され、adapter は値を
/// 保持・表示・エラー文脈化しない。
#[cfg(not(feature = "secrets-internal-test-stub"))]
struct YubikeyPinVerifier<'a>(&'a mut YubiKey);

#[cfg(not(feature = "secrets-internal-test-stub"))]
impl piv_pin::PivPinVerifier for YubikeyPinVerifier<'_> {
    fn verify(&mut self, bytes: &[u8]) -> Result<()> {
        self.0
            .verify_pin(bytes)
            .context("failed to verify YubiKey PIV PIN")
    }
}

/// bytes の SHA-256 digest を lowercase hex 文字列へ整形する（公開鍵 fingerprint 用、秘密情報ではない）。
#[cfg(not(feature = "secrets-internal-test-stub"))]
fn sha256_lowercase_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}
