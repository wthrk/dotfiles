//! YubiKey PIV discovery/selection と実プロセス I/O を port 契約へ接続する adapter。

mod device;
#[cfg(feature = "secrets-test-stub")]
mod device_test_stub;
mod report;
mod secret_io;

use aes_gcm::{Aes256Gcm, KeyInit, aead::AeadInPlace};
use anyhow::bail;

use crate::{
    Result,
    secrets::domain::{
        blob::{CONTENT_KEY_LEN, NONCE_LEN, SecretBlob, TAG_LEN},
        manifest::BootstrapSecretDocument,
        manifest::SecretManifest,
        material::SecretMaterial,
        piv::{PivObjectId, SecretName, StorageObjectIds},
        values::{DeviceCandidate, EnrollSummary, VerifySummary},
    },
    secrets::ports::{
        BootstrapSecretDocumentInputPort, DevicePinPolicyPort, DeviceSelectionPort,
        DeviceSerialPort, PinInputPort, RandomBytesPort, ReportPort, SecretDevice, SecretInputPort,
        SecretLoadPort, SecretOutputPort, SecretStorePort, SpareDeviceSerialPort, StorageSetupPort,
        StorageVerifyPort,
    },
    secrets::support::protection::ProtectedSecret,
};

use self::{
    device::SelectedDeviceAdapter, report::JsonReportAdapter, secret_io::RealSecretIoAdapter,
};

const AEAD_NONCE_LEN: usize = 12;

fn aes_256_gcm_from_key(key: &[u8]) -> Result<Aes256Gcm> {
    Aes256Gcm::new_from_slice(key).map_err(anyhow::Error::new)
}

fn encrypt_detached(
    cipher: &Aes256Gcm,
    nonce: &[u8],
    additional_data: &[u8],
    buffer: &mut [u8],
) -> Result<[u8; TAG_LEN]> {
    if nonce.len() != AEAD_NONCE_LEN {
        bail!("invalid AES-256-GCM nonce length");
    }
    let tag = cipher
        .encrypt_in_place_detached(aes_gcm::Nonce::from_slice(nonce), additional_data, buffer)
        .map_err(|error| anyhow::anyhow!("AES-GCM encrypt failed: {error:?}"))?;
    tag.as_slice().try_into().map_err(anyhow::Error::new)
}

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
    if tag.len() != TAG_LEN {
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
}

/// 実機 device・実プロセス I/O・report 出力を束ねる runtime adapter。
///
/// この型は複数 port の実装を 1 箇所に集約し、application 層へ concrete I/O を漏らさない境界として機能する。
pub(crate) struct RealSecretsBoundary<D = SelectedDeviceAdapter>
where
    D: DeviceSelectionPort,
{
    device: D,
    secret_io: RealSecretIoAdapter,
    report: JsonReportAdapter,
}

impl<D> Default for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort + Default,
{
    fn default() -> Self {
        Self {
            device: D::default(),
            secret_io: RealSecretIoAdapter,
            report: JsonReportAdapter,
        }
    }
}

impl<D> DeviceSelectionPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
    D::Device: SecretDevice,
{
    type Device = D::Device;

    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>> {
        self.device.discover_devices()
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<Self::Device> {
        self.device.open_device_by_serial(serial)
    }
}

impl<D> DeviceSerialPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
{
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

impl<D> SpareDeviceSerialPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
{
    fn resolve_spare_device_serial(
        &mut self,
        primary_serial: Option<u32>,
        requested_spare_serial: Option<u32>,
    ) -> Result<u32> {
        let spare_serial = self.resolve_device_serial(requested_spare_serial)?;
        if primary_serial == Some(spare_serial) {
            bail!("primary and spare YubiKey serial must be different");
        }
        Ok(spare_serial)
    }
}

impl<D> DevicePinPolicyPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
    D::Device: SecretDevice,
{
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool> {
        let device = self.open_device_by_serial(serial)?;
        Ok(device.requires_pin_input())
    }
}

/// PIN 必須デバイスに対してのみ `verify_pin` を実行する。
///
/// PIN の取得手段選択（TTY か stdin か）は caller が担い、
/// この helper は「PIN 必須時に入力が無ければ停止する」境界だけを担う。
fn verify_pin_if_required(
    device: &mut impl SecretDevice,
    pin: Option<&SecretMaterial>,
) -> Result<()> {
    // PIN 要求フラグが false の device では、ここで即 return して追加 I/O を行わない。
    // PIN 要求時に `pin` が `None` だった場合の停止はこの関数が担い、
    // 実際の PIN 値取得（TTY / stdin などの境界選択）は caller 側の責務とする。
    if !device.requires_pin_input() {
        return Ok(());
    }
    let Some(pin) = pin else {
        bail!("PIN is required for this operation");
    };
    device.verify_pin(pin)
}

impl<D> RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
    D::Device: SecretDevice,
{
    /// 指定 serial の device から 1 secret を読み出す。
    ///
    /// PIN が必要なデバイスでは検証を先に実施し、未入力時はここで停止する。
    fn load_secret_from_device(
        &mut self,
        serial: u32,
        name: SecretName,
        pin: Option<&SecretMaterial>,
    ) -> Result<SecretMaterial> {
        let mut device = self.open_device_by_serial(serial)?;
        verify_pin_if_required(&mut device, pin)?;
        SecretManifest::decode_initialized(device.read_object(PivObjectId::MANIFEST)?.as_deref())?;
        let encoded = device
            .read_object(name.object_id())?
            .ok_or_else(|| anyhow::anyhow!("{name} is not stored on this YubiKey"))?;
        let blob = SecretBlob::decode(&encoded)
            .map_err(|error| anyhow::anyhow!("failed to decode {name}: {error}"))?;
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
        let content_key = device.unwrap_key(&wrapped_key)?;
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
                    &blob_name.additional_data(device.serial()),
                    secret_bytes,
                    &tag,
                )
            })
            .map_err(|_| anyhow::anyhow!("failed to decrypt {}", blob_name))?;
        Ok(SecretMaterial::from_vec(secret.into_vec()))
    }

    /// 指定 serial の device へ 1 secret を保存する。
    ///
    /// 上書き可否判定は `SecretDevice::store_secret` の契約へ委譲する。
    fn store_secret_to_device(
        &mut self,
        serial: u32,
        name: SecretName,
        force: bool,
        secret: &SecretMaterial,
    ) -> Result<()> {
        let mut device = self.open_device_by_serial(serial)?;
        secret.with_bytes(|bytes| name.ensure_value_non_empty(bytes))?;
        SecretManifest::decode_initialized(device.read_object(PivObjectId::MANIFEST)?.as_deref())?;
        device.check_management_auth_preconditions()?;
        if device.read_object(name.object_id())?.is_some() && !force {
            bail!("{} already exists; pass --force to replace it", name);
        }
        let mut content_key = ProtectedSecret::new(vec![0u8; CONTENT_KEY_LEN]);
        content_key.with_secret_mut(|value| self.fill_random_bytes(value))?;
        let content_key = SecretMaterial::from_vec(content_key.into_vec());
        let mut nonce = [0u8; NONCE_LEN];
        self.fill_random_bytes(&mut nonce)?;
        let cipher = content_key.with_bytes(aes_256_gcm_from_key)?;
        let mut ciphertext = secret.with_bytes(|bytes| ProtectedSecret::new(bytes.to_vec()));
        let tag = ciphertext.with_secret_mut(|ciphertext_bytes| {
            encrypt_detached(
                &cipher,
                &nonce,
                &name.additional_data(device.serial()),
                ciphertext_bytes,
            )
        })?;
        let wrapped_key = device.wrap_key(&content_key)?;
        let blob = SecretBlob {
            name,
            nonce,
            wrapped_key,
            ciphertext: ciphertext.into_vec(),
            tag,
        };
        let mut encoded = blob.encode()?;
        device.write_object(name.object_id(), &mut encoded)
    }

    /// 指定 serial の storage setup を実行する。
    fn setup_storage_on_device(&mut self, serial: u32) -> Result<()> {
        let mut device = self.open_device_by_serial(serial)?;
        device.check_key_generation_preconditions()?;
        device.check_management_auth_preconditions()?;
        let key_exists = device.key_exists()?;
        let manifest_bytes = device.read_object(PivObjectId::MANIFEST)?;
        let mut occupied_object_ids = Vec::new();
        for object_id in StorageObjectIds::iter() {
            if device.read_object(object_id)?.is_some() {
                occupied_object_ids.push(object_id);
            }
        }
        SecretManifest::ensure_setup_allowed(
            key_exists,
            manifest_bytes.as_deref(),
            &occupied_object_ids,
        )?;
        device.generate_key()?;
        let mut manifest = SecretManifest::expected().encode()?;
        device.write_object(PivObjectId::MANIFEST, &mut manifest)
    }

    /// 指定 serial の local storage 整合を検証する。
    ///
    /// PIN 必須デバイスでは事前検証を通したうえで必須 secret 群の読み出し確認を行う。
    fn verify_local_storage_on_device(
        &mut self,
        serial: u32,
        pin: Option<&SecretMaterial>,
    ) -> Result<()> {
        for name in SecretName::iter() {
            let secret = self.load_secret_from_device(serial, name, pin)?;
            secret.with_bytes(|bytes| name.ensure_value_non_empty(bytes))?;
        }
        Ok(())
    }
}

impl<D> PinInputPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
{
    fn read_pin(&self) -> Result<SecretMaterial> {
        self.secret_io.read_pin()
    }
}

impl<D> SecretInputPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
{
    fn read_visible_secret(&self) -> Result<SecretMaterial> {
        self.secret_io.read_visible_secret()
    }

    fn read_hidden_secret(&self, name: SecretName) -> Result<SecretMaterial> {
        self.secret_io.read_hidden_secret(name)
    }

    fn read_stdin_secret(&self) -> Result<SecretMaterial> {
        self.secret_io.read_stdin_secret()
    }
}

impl<D> BootstrapSecretDocumentInputPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
{
    fn read_bootstrap_secret_document_noninteractive(&self) -> Result<BootstrapSecretDocument> {
        self.secret_io
            .read_bootstrap_secret_document_noninteractive()
    }
}

impl<D> SecretOutputPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
{
    fn write_secret(&self, secret: &SecretMaterial) -> Result<()> {
        self.secret_io.write_secret(secret)
    }
}

impl<D> SecretLoadPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
    D::Device: SecretDevice,
{
    fn load_secret(
        &mut self,
        serial: u32,
        name: SecretName,
        pin: Option<&SecretMaterial>,
    ) -> Result<SecretMaterial> {
        self.load_secret_from_device(serial, name, pin)
    }
}

impl<D> SecretStorePort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
    D::Device: SecretDevice,
{
    fn store_secret(
        &mut self,
        serial: u32,
        name: SecretName,
        force: bool,
        secret: &SecretMaterial,
    ) -> Result<()> {
        self.store_secret_to_device(serial, name, force, secret)
    }
}

impl<D> StorageSetupPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
    D::Device: SecretDevice,
{
    fn setup_storage(&mut self, serial: u32) -> Result<()> {
        self.setup_storage_on_device(serial)
    }
}

impl<D> StorageVerifyPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
    D::Device: SecretDevice,
{
    fn verify_local_storage(&mut self, serial: u32, pin: Option<&SecretMaterial>) -> Result<()> {
        self.verify_local_storage_on_device(serial, pin)
    }
}

impl ReportPort for RealSecretsBoundary<SelectedDeviceAdapter> {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()> {
        self.report
            .write_enroll_report_for_route(summary, self.device.adapter_route_label())
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        self.report
            .write_verify_report_for_route(summary, self.device.adapter_route_label())
    }
}

impl<D> RandomBytesPort for RealSecretsBoundary<D>
where
    D: DeviceSelectionPort,
{
    fn fill_random_bytes(&self, out: &mut [u8]) -> Result<()> {
        use rand::RngCore;
        rand::rng().fill_bytes(out);
        Ok(())
    }
}
