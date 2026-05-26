//! `dotfiles secrets` application 層が外部境界へ要求する port 契約。
//!
//! この module は capability 契約のみを定義し、具体的な parser / 暗号処理 / 端末 I/O /
//! device 操作手順は adapter 側の実装へ閉じ込める。

use std::time::Duration;

use zeroize::Zeroizing;

use crate::Result;

use super::domain::{
    BootstrapSecretDocument, CheckName, EnrollSummary, PivObjectId, SecretName, VerifySummary,
};

pub const SPARE_WAIT_TIMEOUT: Duration = Duration::from_secs(300);
pub const SPARE_DETECT_POLL_INTERVAL: Duration = Duration::from_millis(200);
pub const SPARE_WAIT_TIMEOUT_ERROR: &str = "timed out waiting for spare YubiKey";

/// use case が device 候補列挙と open を要求する capability 契約。
pub trait DeviceSelectionPort {
    type Device: SecretDevice;
    type DeviceCandidate;

    fn discover_devices(&mut self) -> Result<Vec<Self::DeviceCandidate>>;
    fn open_device_by_serial(&mut self, serial: u32) -> Result<Self::Device>;
}

/// 複数候補から対象 device serial を決定する capability 契約。
pub trait DeviceSelectionInputPort: DeviceSelectionPort {
    fn choose_device(&self, devices: &[Self::DeviceCandidate]) -> Result<u32>;
}

/// use case が対象 YubiKey serial を解決するための capability 契約。
pub trait DeviceSerialPort {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32>;
}

/// spare YubiKey の挿入待ちを処理する capability 契約。
pub trait SpareDeviceWaitPort {
    fn wait_for_spare_device(&self) -> Result<()>;
}

/// use case が primary と衝突しない spare YubiKey serial を解決する契約。
pub trait SpareDeviceSerialPort {
    fn resolve_spare_device_serial(
        &mut self,
        primary_serial: Option<u32>,
        spare_serial: Option<u32>,
    ) -> Result<u32>;
}

/// use case が PIV PIN を取得するための capability 契約。
pub trait PinInputPort {
    fn read_pin(&self) -> Result<Zeroizing<Vec<u8>>>;
}

/// use case が必要とする secret 入力 capability 契約。
pub trait SecretInputPort {
    fn read_visible_secret(&self, label: &str) -> Result<Zeroizing<Vec<u8>>>;
    fn read_hidden_secret(&self, label: &str) -> Result<Zeroizing<Vec<u8>>>;
    fn read_stdin_secret(&self) -> Result<Zeroizing<Vec<u8>>>;
    fn read_secret_document_noninteractive(&self) -> Result<Zeroizing<Vec<u8>>>;
    fn read_bootstrap_secret_document(&self) -> Result<BootstrapSecretDocument>;
}

/// use case が復号済み secret を出力境界へ渡す契約。
pub trait SecretOutputPort {
    fn write_secret(&self, bytes: &[u8]) -> Result<()>;
}

/// use case が保存済み secret を読み出すための契約。
pub trait SecretLoadPort {
    fn load_secret(&mut self, serial: u32, name: SecretName) -> Result<Zeroizing<Vec<u8>>>;
}

/// use case が secret を保存するための契約。
pub trait SecretStorePort {
    fn store_secret(
        &mut self,
        serial: u32,
        name: SecretName,
        force: bool,
        secret: &[u8],
    ) -> Result<()>;
}

/// use case が YubiKey storage layout を初期化するための契約。
pub trait StorageSetupPort {
    fn setup_storage(&mut self, serial: u32) -> Result<()>;
}

/// use case が bootstrap secret 文書を読み出すための契約。
pub trait BootstrapSecretLoadPort {
    fn load_bootstrap_secret_document(&mut self, serial: u32) -> Result<BootstrapSecretDocument>;
}

/// use case が bootstrap secret 文書を保存するための契約。
pub trait BootstrapSecretStorePort {
    fn store_bootstrap_secret_document(
        &mut self,
        serial: u32,
        document: &BootstrapSecretDocument,
    ) -> Result<()>;
}

/// use case が local storage の整合性を検証する契約。
pub trait StorageVerifyPort {
    fn verify_local_storage(&mut self, serial: u32) -> Result<()>;
}

/// use case が結果報告を出力境界へ渡すための契約。
pub trait ReportPort {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()>;
    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()>;
    fn report_primary_enrollment(&self, serial: u32) -> Result<()>;
    fn report_spare_enrollment(&self, serial: u32) -> Result<()>;
    fn report_local_storage_verified(&self, serial: u32) -> Result<()>;
    fn report_local_storage_failed(&self, serial: u32) -> Result<()>;
    fn report_external_checks_unavailable(
        &self,
        serial: u32,
        checks: impl IntoIterator<Item = CheckName>,
    ) -> Result<()>;
}

/// use case が鍵素材生成に必要な乱数を要求する契約。
pub trait RandomBytesPort {
    fn fill_random_bytes(&self, out: &mut [u8]) -> Result<()>;
}

/// YubiKey device adapter が満たす低水準 device 操作契約。
pub trait SecretDevice {
    fn serial(&self) -> u32;
    fn key_exists(&mut self) -> Result<bool>;
    fn check_key_generation_preconditions(&mut self) -> Result<()>;
    fn check_management_auth_preconditions(&mut self) -> Result<()>;
    fn generate_key(&mut self) -> Result<()>;
    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>>;
    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()>;
    fn wrap_key(&mut self, key: &[u8]) -> Result<Vec<u8>>;
    fn requires_pin_input(&self) -> bool;
    fn verify_pin(&mut self, pin: &[u8]) -> Result<()>;
    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Zeroizing<Vec<u8>>>;
}
