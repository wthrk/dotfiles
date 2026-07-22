//! YubiKey PIV の device discovery、slot I/O、secret-protection bridge。
//!
//! この module は YubiKey crate handle と technical state を所有する。port trait 実装は
//! adapter にのみ置く。
//!
//! ## 出典と適用判断
//!
//! repository の復旧契約と PIV 保存対象は
//! [`secret-recovery-spec.md`](../../../docs/secret-recovery/secret-recovery-spec.md) の
//! 「無対話復旧の利用者契約」および「責務分担 / YubiKey」、保存形式は
//! [`yubikey-secret-storage-design.md`](../../../docs/secret-recovery/yubikey-secret-storage-design.md)
//! の「PIV 領域」を正本とする。この module はその保存・復号の**技術的** backend だけを
//! 実装し、secret の必須性、対象名の一意解決、復旧手順、成功/停止条件を決めない。
//!
//! vendor の全体像は [YubiKey Technical Manual の PIV section](https://docs.yubico.com/hardware/yubikey/yk-tech-manual/yk5-apps-piv.html)
//! （PIV application、slot 82--95、PIN/touch policy、metadata）を読む。実際に直接使う
//! `yubikey` 0.9.0-pre.0 API は version 固定の upstream source
//! [`YubiKey::open_by_serial` / `YubiKey::authenticate` / `YubiKey::verify_pin`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/yubikey.rs)、
//! [`piv::generate` / `piv::metadata` / `piv::decrypt_data`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/piv.rs)、
//! [`Transaction::get_metadata` / `Transaction::fetch_object` / `Transaction::save_object`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/transaction.rs)
//! である。適用判断は API ごとに限定する。`fetch_object` の `Error::NotFound` だけを下記
//! `read_object` で object absence へ翻訳する。`piv::metadata` 経由の
//! `Transaction::get_metadata` の `Error::NotFound` は、それ自体を absence の成功にせず
//! certificate を追加観測する契機にだけ使う。他の `yubikey::Error` は意味を推測せず
//! source error のまま伝播する。実機観測をこの判断の根拠にはしない。

#[cfg(not(feature = "secrets-internal-test-stub"))]
use crate::support::{
    piv_storage::sha256_lowercase_hex,
    protection::{sealed_blob, secret_random, yubikey_piv},
};
use crate::{
    Result,
    domain::{
        gpg_backup::{ConnectedYubiKey, EnvelopeRecipient},
        piv::{PivApplicationVersion, PivObjectId, SecretStorageSpec},
    },
    support::protection::{ProtectedSecret, SecretSession},
};
#[cfg(not(feature = "secrets-internal-test-stub"))]
use anyhow::Context;
#[cfg(not(feature = "secrets-internal-test-stub"))]
use rsa::{
    RsaPublicKey,
    pkcs1::DecodeRsaPublicKey,
    pkcs8::{DecodePublicKey, EncodePublicKey},
};
#[cfg(not(feature = "secrets-internal-test-stub"))]
use yubikey::{
    Context as YubikeyContext, PinPolicy, Serial, TouchPolicy, YubiKey,
    piv::{self, AlgorithmId, RetiredSlotId, SlotId},
};

#[derive(Default)]
pub(crate) struct YubikeyDeviceBackend;
#[derive(Default)]
pub(crate) struct YubikeyRecipientBackend;
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceCandidate {
    pub(crate) serial: u32,
    pub(crate) label: String,
    pub(crate) piv_version: PivApplicationVersion,
}
pub(crate) trait SecretDeviceIo {
    fn key_exists(&mut self) -> Result<bool>;
    fn reserved_slot_certificate_exists(&mut self) -> Result<bool>;
    fn piv_application_version(&self) -> PivApplicationVersion;
    fn verify_management_pin(&mut self, pin: &ProtectedSecret) -> Result<()>;
    fn authenticate_protected_management_key(&mut self) -> Result<()>;
    fn generate_key(&mut self) -> Result<Vec<u8>>;
    fn slot_public_key_spki(&mut self) -> Result<Option<Vec<u8>>>;
    fn remember_generated_public_key(&mut self, key: Vec<u8>);
    fn read_object(&mut self, object: PivObjectId) -> Result<Option<Vec<u8>>>;
    fn write_object(&mut self, object: PivObjectId, value: &mut [u8]) -> Result<()>;
    fn empty_object(&mut self, object: PivObjectId) -> Result<()>;
    fn clear_reserved_slot_certificate(&mut self) -> Result<()>;
    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        secret: &ProtectedSecret,
    ) -> Result<Vec<u8>>;
    fn open_from_storage(
        &mut self,
        storage: SecretStorageSpec,
        encoded: &[u8],
    ) -> Result<ProtectedSecret>;
    fn recipient_public_key_fingerprint(&mut self) -> Result<String>;
    fn wrap_dek(&mut self, dek: &ProtectedSecret) -> Result<Vec<u8>>;
    fn unwrap_dek(&mut self, wrapped: &[u8]) -> Result<ProtectedSecret>;
}
pub(crate) struct SelectedSecretDevice {
    inner: Box<dyn SecretDeviceIo>,
}
impl SelectedSecretDevice {
    pub(crate) fn new(device: impl SecretDeviceIo + 'static) -> Self {
        Self {
            inner: Box::new(device),
        }
    }
}
macro_rules! delegate { ($name:ident($($arg:ident:$typ:ty),*) -> $ret:ty) => { fn $name(&mut self,$($arg:$typ),*) -> $ret { self.inner.$name($($arg),*) } }; }
impl SecretDeviceIo for SelectedSecretDevice {
    delegate!(key_exists() -> Result<bool>);
    delegate!(reserved_slot_certificate_exists() -> Result<bool>);
    fn piv_application_version(&self) -> PivApplicationVersion {
        self.inner.piv_application_version()
    }
    delegate!(verify_management_pin(pin:&ProtectedSecret) -> Result<()>);
    delegate!(authenticate_protected_management_key() -> Result<()>);
    delegate!(generate_key() -> Result<Vec<u8>>);
    delegate!(slot_public_key_spki() -> Result<Option<Vec<u8>>>);
    fn remember_generated_public_key(&mut self, key: Vec<u8>) {
        self.inner.remember_generated_public_key(key)
    }
    delegate!(read_object(object:PivObjectId) -> Result<Option<Vec<u8>>>);
    delegate!(write_object(object:PivObjectId,value:&mut [u8]) -> Result<()>);
    delegate!(empty_object(object:PivObjectId) -> Result<()>);
    delegate!(clear_reserved_slot_certificate() -> Result<()>);
    delegate!(seal_for_storage(storage:SecretStorageSpec,secret:&ProtectedSecret) -> Result<Vec<u8>>);
    delegate!(open_from_storage(storage:SecretStorageSpec,encoded:&[u8]) -> Result<ProtectedSecret>);
    delegate!(recipient_public_key_fingerprint() -> Result<String>);
    delegate!(wrap_dek(dek:&ProtectedSecret) -> Result<Vec<u8>>);
    delegate!(unwrap_dek(wrapped:&[u8]) -> Result<ProtectedSecret>);
}

pub(crate) fn discover_devices(_: &mut YubikeyDeviceBackend) -> Result<Vec<DeviceCandidate>> {
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    {
        let mut context = YubikeyContext::open()?;
        let mut devices = Vec::new();
        for reader in context.iter()? {
            let label = reader.name().into_owned();
            let key = reader.open()?;
            devices.push(DeviceCandidate {
                serial: key.serial().0,
                label,
                piv_version: PivApplicationVersion {
                    major: key.version().major,
                    minor: key.version().minor,
                    patch: key.version().patch,
                },
            });
        }
        Ok(devices)
    }
    #[cfg(feature = "secrets-internal-test-stub")]
    {
        crate::support::internal_stub_yubikey::discover_devices()
    }
}
pub(crate) fn open_device_by_serial(
    _: &mut YubikeyDeviceBackend,
    serial: u32,
) -> Result<SelectedSecretDevice> {
    #[cfg(not(feature = "secrets-internal-test-stub"))]
    {
        Ok(SelectedSecretDevice::new(YubikeySecretDevice {
            yubikey: YubiKey::open_by_serial(Serial(serial))?,
            generated_public_key: None,
        }))
    }
    #[cfg(feature = "secrets-internal-test-stub")]
    {
        crate::support::internal_stub_yubikey::open_device_by_serial(serial)
    }
}
pub(crate) fn open_recipient_device(
    _: &mut YubikeyRecipientBackend,
    serial: u32,
) -> Result<SelectedSecretDevice> {
    open_device_by_serial(&mut YubikeyDeviceBackend, serial)
}
pub(crate) fn resolve_connected_recipient(
    backend: &mut YubikeyRecipientBackend,
    serial: u32,
) -> Result<ConnectedYubiKey> {
    let mut device = open_recipient_device(backend, serial)?;
    ConnectedYubiKey::new(
        serial.to_string(),
        &device.recipient_public_key_fingerprint()?,
    )
}
pub(crate) fn wrap_dek_for_recipient(
    backend: &mut YubikeyRecipientBackend,
    serial: u32,
    dek: &ProtectedSecret,
) -> Result<EnvelopeRecipient> {
    let mut device = open_recipient_device(backend, serial)?;
    let connected = ConnectedYubiKey::new(
        serial.to_string(),
        &device.recipient_public_key_fingerprint()?,
    )?;
    EnvelopeRecipient::new(&connected, device.wrap_dek(dek)?)
}
pub(crate) fn unwrap_dek(
    backend: &mut YubikeyRecipientBackend,
    serial: u32,
    recipient: &EnvelopeRecipient,
) -> Result<ProtectedSecret> {
    let _session = SecretSession::start()?;
    let mut device = open_recipient_device(backend, serial)?;
    device.unwrap_dek(recipient.wrapped_dek())
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
const SECRET_SLOT: SlotId = SlotId::Retired(RetiredSlotId::R1);
#[cfg(not(feature = "secrets-internal-test-stub"))]
const SECRET_SLOT_CERT_OBJECT_ID: u32 = 0x005f_c10d;
#[cfg(not(feature = "secrets-internal-test-stub"))]
struct YubikeySecretDevice {
    yubikey: YubiKey,
    generated_public_key: Option<Vec<u8>>,
}
#[cfg(not(feature = "secrets-internal-test-stub"))]
impl YubikeySecretDevice {
    fn slot_public_key_spki_from_metadata(&mut self) -> Result<Option<Vec<u8>>> {
        piv::metadata(&mut self.yubikey, SECRET_SLOT)?
            .public
            .map(|p| {
                RsaPublicKey::from_pkcs1_der(p.subject_public_key.raw_bytes())
                    .context("failed to parse YubiKey slot 82 metadata public key")?
                    .to_public_key_der()
                    .context("failed to DER-encode YubiKey slot 82 metadata public key")
                    .map(|v| v.as_bytes().to_vec())
            })
            .transpose()
    }
    fn slot_public_key(&mut self) -> Result<RsaPublicKey> {
        if let Some(key) = self.generated_public_key.as_deref() {
            return RsaPublicKey::from_public_key_der(key)
                .context("failed to parse cached YubiKey secret storage public key");
        }
        let key = self
            .slot_public_key_spki()?
            .ok_or_else(|| anyhow::anyhow!("YubiKey secret storage key metadata is unavailable"))?;
        RsaPublicKey::from_public_key_der(&key)
            .context("failed to parse YubiKey secret storage public key")
    }
    fn wrap_content_key(&mut self, key: &ProtectedSecret) -> Result<Vec<u8>> {
        secret_random::rsa_oaep_encrypt(&self.slot_public_key()?, key)
    }
    fn unwrap_content_key(&mut self, wrapped: &[u8]) -> Result<ProtectedSecret> {
        sealed_blob::unwrap_content_key_from_decrypt(
            || {
                piv::decrypt_data(
                    &mut self.yubikey,
                    wrapped,
                    AlgorithmId::Rsa2048,
                    SECRET_SLOT,
                )
                .map_err(anyhow::Error::new)
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
            Err(yubikey::Error::NotFound) => self.reserved_slot_certificate_exists(),
            Err(error) => Err(error.into()),
        }
    }
    fn reserved_slot_certificate_exists(&mut self) -> Result<bool> {
        match self.yubikey.fetch_object(SECRET_SLOT_CERT_OBJECT_ID) {
            Ok(value) => Ok(!value.is_empty()),
            Err(yubikey::Error::NotFound) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }
    fn piv_application_version(&self) -> PivApplicationVersion {
        let v = self.yubikey.version();
        PivApplicationVersion {
            major: v.major,
            minor: v.minor,
            patch: v.patch,
        }
    }
    fn verify_management_pin(&mut self, pin: &ProtectedSecret) -> Result<()> {
        yubikey_piv::verify_pin(&mut self.yubikey, pin)
    }
    fn authenticate_protected_management_key(&mut self) -> Result<()> {
        let key = yubikey::MgmKey::get_protected(&mut self.yubikey).map_err(anyhow::Error::new)?;
        self.yubikey
            .authenticate(&key)
            .map_err(anyhow::Error::new)?;
        let metadata = piv::metadata(
            &mut self.yubikey,
            SlotId::Management(yubikey::piv::ManagementSlotId::Management),
        )?;
        if metadata.default != Some(false) {
            anyhow::bail!("YubiKey PIN-protected management key metadata is not healthy")
        }
        Ok(())
    }
    fn generate_key(&mut self) -> Result<Vec<u8>> {
        let public = piv::generate(
            &mut self.yubikey,
            SECRET_SLOT,
            AlgorithmId::Rsa2048,
            PinPolicy::Never,
            TouchPolicy::Always,
        )?;
        let encoded = RsaPublicKey::from_pkcs1_der(public.subject_public_key.raw_bytes())
            .context("failed to parse generated YubiKey secret storage public key")?
            .to_public_key_der()
            .context("failed to DER-encode generated YubiKey secret storage public key")?
            .as_bytes()
            .to_vec();
        self.generated_public_key = Some(encoded.clone());
        Ok(encoded)
    }
    fn slot_public_key_spki(&mut self) -> Result<Option<Vec<u8>>> {
        self.slot_public_key_spki_from_metadata()
    }
    fn remember_generated_public_key(&mut self, key: Vec<u8>) {
        self.generated_public_key = Some(key)
    }
    /// custom object を読み、crate が定義する absence だけを `None` にする。
    ///
    /// 出典: repository 正本は
    /// [`yubikey-secret-storage-design.md` の「Object IDs」](../../../docs/secret-recovery/yubikey-secret-storage-design.md#object-ids)、
    /// vendor / SDK の正確な根拠は `yubikey` 0.9.0-pre.0
    /// [`Transaction::fetch_object`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/transaction.rs)
    /// （`StatusWords::NotFoundError` を `Error::NotFound` にする分岐）である。
    /// 適用判断: `Error::NotFound` だけを object absence として `None` にし、成功した
    /// zero-length payload は physical object が存在する `Some(vec![])` のまま保持する。
    /// そのほかの error は status、permission、device state 等へ再分類せず伝播する。
    fn read_object(&mut self, object: PivObjectId) -> Result<Option<Vec<u8>>> {
        match self.yubikey.fetch_object(object.value()) {
            Ok(value) => Ok(Some(value.to_vec())),
            Err(yubikey::Error::NotFound) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    fn write_object(&mut self, object: PivObjectId, value: &mut [u8]) -> Result<()> {
        self.yubikey.save_object(object.value(), value)?;
        Ok(())
    }
    fn empty_object(&mut self, object: PivObjectId) -> Result<()> {
        self.write_object(object, &mut [])
    }
    fn clear_reserved_slot_certificate(&mut self) -> Result<()> {
        self.yubikey
            .save_object(SECRET_SLOT_CERT_OBJECT_ID, &mut [])?;
        self.generated_public_key = None;
        Ok(())
    }
    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        secret: &ProtectedSecret,
    ) -> Result<Vec<u8>> {
        sealed_blob::seal_material_with_key_wrap(
            storage.secret_id,
            secret,
            &storage.additional_data,
            |key| self.wrap_content_key(key),
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
            |wrapped| self.unwrap_content_key(wrapped),
            &storage.additional_data,
        )
    }
    fn recipient_public_key_fingerprint(&mut self) -> Result<String> {
        let der = self
            .slot_public_key()?
            .to_public_key_der()
            .context("failed to DER-encode YubiKey slot 82 public key")?;
        Ok(sha256_lowercase_hex(der.as_bytes()))
    }
    fn wrap_dek(&mut self, dek: &ProtectedSecret) -> Result<Vec<u8>> {
        self.wrap_content_key(dek)
    }
    fn unwrap_dek(&mut self, wrapped: &[u8]) -> Result<ProtectedSecret> {
        self.unwrap_content_key(wrapped)
    }
}
