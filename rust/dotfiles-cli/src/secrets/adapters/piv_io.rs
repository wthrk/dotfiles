//! YubiKey PIV discovery/selection と実プロセス I/O を port 契約へ接続する adapter。

mod console_io;
mod device;
mod report;
mod secret_io;

use anyhow::{Context, anyhow, bail};
use zeroize::Zeroizing;

use crate::{
    Result,
    secrets::adapters::yubikey::YubikeySecretDevice,
    secrets::domain::{
        BootstrapSecretDocument, CONTENT_KEY_LEN, CheckName, EnrollSummary, NONCE_LEN, PivObjectId,
        SecretBlob, SecretManifest, SecretName, StorageObjectIds, VerifySummary,
        aes_256_gcm_from_key, decode_initialized_manifest, decrypt_detached, encode_manifest,
        encrypt_detached, ensure_secret_value_non_empty,
    },
    secrets::ports::{
        BootstrapSecretLoadPort, BootstrapSecretStorePort, DeviceSelectionInputPort,
        DeviceSelectionPort, DeviceSerialPort, PinInputPort, RandomBytesPort, ReportPort,
        SecretDevice, SecretInputPort, SecretLoadPort, SecretOutputPort, SecretStorePort,
        SpareDeviceSerialPort, SpareDeviceWaitPort, StorageSetupPort, StorageVerifyPort,
    },
};

use self::{
    device::{DiscoveredDevice, RealDeviceAdapter},
    report::JsonReportAdapter,
    secret_io::RealSecretIoAdapter,
};

/// 実機 device・実プロセス I/O・report 出力を束ねる runtime adapter。
pub struct RealSecretsBoundary {
    device: RealDeviceAdapter,
    secret_io: RealSecretIoAdapter,
    report: JsonReportAdapter,
}

impl Default for RealSecretsBoundary {
    fn default() -> Self {
        Self {
            device: RealDeviceAdapter,
            secret_io: RealSecretIoAdapter,
            report: JsonReportAdapter,
        }
    }
}

impl RealSecretsBoundary {
    fn open_device(&mut self, serial: u32) -> Result<YubikeySecretDevice> {
        self.device.open_device_by_serial(serial)
    }

    fn choose_device(&self, devices: &[DiscoveredDevice]) -> Result<u32> {
        match devices {
            [] => bail!("no YubiKey detected"),
            [device] => Ok(device.serial),
            _ => {
                eprintln!("Multiple YubiKeys detected:");
                for (index, device) in devices.iter().enumerate() {
                    eprintln!(
                        "  {}: {} (serial {})",
                        index + 1,
                        device.label,
                        device.serial
                    );
                }
                let selection = console_io::read_prompt_line("Select YubiKey by number: ")?;
                let selected_index = selection
                    .trim()
                    .parse::<usize>()
                    .context("device selection must be a number")?;
                let serial = devices
                    .get(selected_index.saturating_sub(1))
                    .context("device selection out of range")?
                    .serial;
                Ok(serial)
            }
        }
    }

    fn with_device<T>(
        &mut self,
        serial: u32,
        operation: impl FnOnce(&mut YubikeySecretDevice, &mut Self) -> Result<T>,
    ) -> Result<T> {
        let mut device = self.open_device(serial)?;
        operation(&mut device, self)
    }

    /// 読み出し系処理の前に PIN 検証を強制し、秘密値復号を許可する。
    fn ensure_pin_verified(&self, device: &mut YubikeySecretDevice) -> Result<()> {
        if device.requires_pin_input() {
            let pin = self.read_pin()?;
            device.verify_pin(pin.as_ref())?;
        }
        Ok(())
    }

    fn with_verified_device<T>(
        &mut self,
        serial: u32,
        operation: impl FnOnce(&mut YubikeySecretDevice, &mut Self) -> Result<T>,
    ) -> Result<T> {
        self.with_device(serial, |device, boundary| {
            boundary.ensure_pin_verified(device)?;
            operation(device, boundary)
        })
    }

    /// manifest の存在と format 一致を確認し、初期化済み storage として扱えることを保証する。
    fn ensure_storage_initialized(device: &mut YubikeySecretDevice) -> Result<SecretManifest> {
        let manifest_bytes = device.read_object(PivObjectId::MANIFEST)?;
        decode_initialized_manifest(manifest_bytes.as_deref())
    }

    /// secret storage layout が未初期化であることを確認した上で初期化を実行する。
    fn setup_storage_on_device(device: &mut YubikeySecretDevice) -> Result<()> {
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
        crate::secrets::domain::ensure_storage_setup_allowed(
            key_exists,
            manifest_bytes.as_deref(),
            &occupied_object_ids,
        )?;

        device.generate_key()?;
        let mut manifest = encode_manifest(&SecretManifest::expected())?;
        device.write_object(PivObjectId::MANIFEST, &mut manifest)
    }

    /// YubiKey storage へ 1 secret を暗号化保存する。
    fn store_secret_on_device(
        device: &mut YubikeySecretDevice,
        random: &impl RandomBytesPort,
        name: SecretName,
        secret: &[u8],
        force: bool,
    ) -> Result<()> {
        ensure_secret_value_non_empty(name, secret)?;
        Self::ensure_storage_initialized(device)?;
        device.check_management_auth_preconditions()?;
        if device.read_object(name.object_id())?.is_some() && !force {
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
            &name.additional_data(device.serial()),
            ciphertext.as_mut_slice(),
        )?;
        let wrapped_key = device.wrap_key(content_key.as_ref())?;
        let blob = SecretBlob {
            name,
            nonce,
            wrapped_key,
            ciphertext: ciphertext.to_vec(),
            tag,
        };

        let mut encoded = blob.encode()?;
        device.write_object(name.object_id(), &mut encoded)
    }

    /// YubiKey storage から 1 secret を復号し、zeroizing buffer として返す。
    fn load_secret_from_device(
        device: &mut YubikeySecretDevice,
        name: SecretName,
    ) -> Result<Zeroizing<Vec<u8>>> {
        Self::ensure_storage_initialized(device)?;
        let encoded = device
            .read_object(name.object_id())?
            .with_context(|| format!("{} is not stored on this YubiKey", name))?;
        let blob =
            SecretBlob::decode(&encoded).with_context(|| format!("failed to decode {}", name))?;
        if blob.name != name {
            bail!("YubiKey secret blob name does not match requested {}", name);
        }

        let content_key = device.unwrap_key(&blob.wrapped_key)?;
        if content_key.len() != CONTENT_KEY_LEN {
            bail!("unwrapped YubiKey content key has invalid length");
        }

        let cipher = aes_256_gcm_from_key(&content_key)?;
        let mut secret = Zeroizing::new(blob.ciphertext.clone());
        decrypt_detached(
            &cipher,
            &blob.nonce,
            &blob.name.additional_data(device.serial()),
            secret.as_mut_slice(),
            &blob.tag,
        )
        .map_err(|_| anyhow!("failed to decrypt {}", blob.name))?;
        Ok(secret)
    }

    fn store_bootstrap_secret_document_on_device(
        device: &mut YubikeySecretDevice,
        random: &impl RandomBytesPort,
        document: &BootstrapSecretDocument,
    ) -> Result<()> {
        // 追加コピーを避け、document 文字列の byte slice をそのまま暗号化経路へ渡す。
        Self::store_secret_on_device(
            device,
            random,
            SecretName::BwEmail,
            document.bw_email.as_bytes(),
            false,
        )?;
        Self::store_secret_on_device(
            device,
            random,
            SecretName::BwPassword,
            document.bw_password.as_bytes(),
            false,
        )?;
        Self::store_secret_on_device(
            device,
            random,
            SecretName::BwsAccessToken,
            document.bws_access_token.as_bytes(),
            false,
        )
    }

    fn verify_required_secrets_on_device(device: &mut YubikeySecretDevice) -> Result<()> {
        for name in SecretName::iter() {
            let secret = Self::load_secret_from_device(device, name)?;
            ensure_secret_value_non_empty(name, secret.as_ref())?;
        }
        Ok(())
    }
}

impl DeviceSerialPort for RealSecretsBoundary {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
        match requested {
            Some(serial) => Ok(serial),
            None => {
                let devices = self.device.discover_devices()?;
                self.choose_device(&devices)
            }
        }
    }
}

impl DeviceSelectionPort for RealSecretsBoundary {
    type Device = YubikeySecretDevice;
    type DeviceCandidate = DiscoveredDevice;

    fn discover_devices(&mut self) -> Result<Vec<Self::DeviceCandidate>> {
        self.device.discover_devices()
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<Self::Device> {
        self.device.open_device_by_serial(serial)
    }
}

impl DeviceSelectionInputPort for RealSecretsBoundary {
    fn choose_device(&self, devices: &[Self::DeviceCandidate]) -> Result<u32> {
        RealSecretsBoundary::choose_device(self, devices)
    }
}

impl SpareDeviceSerialPort for RealSecretsBoundary {
    fn resolve_spare_device_serial(
        &mut self,
        primary_serial: Option<u32>,
        spare_serial: Option<u32>,
    ) -> Result<u32> {
        if let Some(serial) = spare_serial {
            if Some(serial) == primary_serial {
                bail!("primary and spare YubiKey serial must be different");
            }
            return Ok(serial);
        }

        loop {
            let devices = self.device.discover_devices()?;
            let serial = self.choose_device(&devices)?;
            if Some(serial) != primary_serial {
                return Ok(serial);
            }
            self.wait_for_spare_device()?;
        }
    }
}

impl SpareDeviceWaitPort for RealSecretsBoundary {
    fn wait_for_spare_device(&self) -> Result<()> {
        let _ = console_io::read_prompt_line("Insert spare YubiKey and press Enter to continue: ")?;
        Ok(())
    }
}

impl PinInputPort for RealSecretsBoundary {
    fn read_pin(&self) -> Result<zeroize::Zeroizing<Vec<u8>>> {
        self.secret_io.read_pin()
    }
}

impl SecretInputPort for RealSecretsBoundary {
    fn read_visible_secret(&self, label: &str) -> Result<zeroize::Zeroizing<Vec<u8>>> {
        self.secret_io.read_visible_secret(label)
    }

    fn read_hidden_secret(&self, label: &str) -> Result<zeroize::Zeroizing<Vec<u8>>> {
        self.secret_io.read_hidden_secret(label)
    }

    fn read_stdin_secret(&self) -> Result<zeroize::Zeroizing<Vec<u8>>> {
        self.secret_io.read_stdin_secret()
    }

    fn read_secret_document_noninteractive(&self) -> Result<zeroize::Zeroizing<Vec<u8>>> {
        self.secret_io.read_secret_document_noninteractive()
    }

    fn read_bootstrap_secret_document(&self) -> Result<BootstrapSecretDocument> {
        self.secret_io.read_bootstrap_secret_document()
    }
}

impl SecretOutputPort for RealSecretsBoundary {
    fn write_secret(&self, bytes: &[u8]) -> Result<()> {
        self.secret_io.write_secret(bytes)
    }
}

impl SecretLoadPort for RealSecretsBoundary {
    fn load_secret(
        &mut self,
        serial: u32,
        name: SecretName,
    ) -> Result<zeroize::Zeroizing<Vec<u8>>> {
        self.with_verified_device(serial, |device, _| {
            Self::load_secret_from_device(device, name)
        })
    }
}

impl SecretStorePort for RealSecretsBoundary {
    fn store_secret(
        &mut self,
        serial: u32,
        name: SecretName,
        force: bool,
        secret: &[u8],
    ) -> Result<()> {
        self.with_device(serial, |device, boundary| {
            Self::store_secret_on_device(device, boundary, name, secret, force)
        })
    }
}

impl StorageSetupPort for RealSecretsBoundary {
    fn setup_storage(&mut self, serial: u32) -> Result<()> {
        self.with_device(serial, |device, _| Self::setup_storage_on_device(device))
    }
}

impl BootstrapSecretLoadPort for RealSecretsBoundary {
    fn load_bootstrap_secret_document(&mut self, serial: u32) -> Result<BootstrapSecretDocument> {
        self.with_verified_device(serial, |device, _| {
            let bw_email = Self::load_secret_from_device(device, SecretName::BwEmail)?;
            let bw_password = Self::load_secret_from_device(device, SecretName::BwPassword)?;
            let bws_access_token =
                Self::load_secret_from_device(device, SecretName::BwsAccessToken)?;
            BootstrapSecretDocument::from_interactive_secrets(
                bw_email.as_ref(),
                bw_password.as_ref(),
                bws_access_token.as_ref(),
            )
        })
    }
}

impl BootstrapSecretStorePort for RealSecretsBoundary {
    fn store_bootstrap_secret_document(
        &mut self,
        serial: u32,
        document: &BootstrapSecretDocument,
    ) -> Result<()> {
        self.with_device(serial, |device, boundary| {
            Self::store_bootstrap_secret_document_on_device(device, boundary, document)
        })
    }
}

impl StorageVerifyPort for RealSecretsBoundary {
    fn verify_local_storage(&mut self, serial: u32) -> Result<()> {
        self.with_verified_device(serial, |device, _| {
            Self::verify_required_secrets_on_device(device)
        })
    }
}

impl ReportPort for RealSecretsBoundary {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()> {
        self.report.write_enroll_report(summary)
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        self.report.write_verify_report(summary)
    }

    fn report_primary_enrollment(&self, serial: u32) -> Result<()> {
        self.write_enroll_report(&EnrollSummary::primary_completed(serial))
    }

    fn report_spare_enrollment(&self, serial: u32) -> Result<()> {
        self.write_enroll_report(&EnrollSummary::spare_completed(serial))
    }

    fn report_local_storage_verified(&self, serial: u32) -> Result<()> {
        self.write_verify_report(&VerifySummary::local_storage_verified(serial))
    }

    fn report_local_storage_failed(&self, serial: u32) -> Result<()> {
        self.write_verify_report(&VerifySummary::local_storage_failed(serial))
    }

    fn report_external_checks_unavailable(
        &self,
        serial: u32,
        checks: impl IntoIterator<Item = CheckName>,
    ) -> Result<()> {
        self.write_verify_report(&VerifySummary::external_checks_unavailable(serial, checks))
    }
}

impl RandomBytesPort for RealSecretsBoundary {
    fn fill_random_bytes(&self, out: &mut [u8]) -> Result<()> {
        use rand::RngCore;
        rand::rng().fill_bytes(out);
        Ok(())
    }
}
