//! `dotfiles secrets` application 層が外部境界へ要求する port 契約。
//!
//! この module は capability 契約と境界データのみを定義し、処理手順や変換規則は持たない。

use super::domain::{
    manifest::BootstrapSecretDocument,
    material::SecretMaterial,
    piv::{SecretName, SecretStorageSpec},
    values::{EnrollSummary, VerifySummary},
};
use anyhow::Result;

/// 対話選択に提示する YubiKey 候補の port 境界データ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCandidate {
    pub serial: u32,
    pub label: String,
}

/// use case が primary 対象の serial を確定する capability 契約。
pub trait DeviceSerialPort {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32>;
}

/// use case が対象 device の PIN 要否を判定する capability 契約。
pub trait DevicePinPolicyPort {
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool>;
}

/// use case が spare 対象の serial を確定する capability 契約。
pub trait SpareDeviceSerialPort {
    fn resolve_spare_device_serial(&mut self, requested_spare_serial: Option<u32>) -> Result<u32>;
}

/// use case が PIV PIN を取得するための capability 契約。
pub trait PinInputPort {
    fn read_pin(&self) -> Result<SecretMaterial>;
}

/// use case が必要とする secret 入力 capability 契約。
pub trait SecretInputPort {
    fn read_visible_secret(&self) -> Result<SecretMaterial>;
    fn read_hidden_secret(&self, name: SecretName) -> Result<SecretMaterial>;
    fn read_stdin_secret(&self) -> Result<SecretMaterial>;
}

/// stdin JSON から bootstrap secret 文書を取得する capability 契約。
pub trait BootstrapSecretDocumentInputPort {
    fn read_bootstrap_secret_document_noninteractive(&self) -> Result<BootstrapSecretDocument>;
}

/// use case が復号済み secret を出力境界へ渡す契約。
pub trait SecretOutputPort {
    fn write_secret(&self, secret: &SecretMaterial) -> Result<()>;
}

/// use case が結果報告を出力境界へ渡すための契約。
pub trait ReportPort {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()>;
    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()>;
}

/// use case が YubiKey secret storage へ要求する高水準 capability 契約。
pub trait SecretStoragePort {
    /// 対象 serial の secret storage を初期化する。
    fn initialize_secret_storage(&mut self, serial: u32) -> Result<()>;
    /// 対象 storage spec の secret を保存する。
    fn store_secret(
        &mut self,
        serial: u32,
        storage: SecretStorageSpec,
        secret: &SecretMaterial,
    ) -> Result<()>;
    /// 対象 storage spec の既存値ポリシーを確認したうえで secret を保存する。
    fn put_secret(
        &mut self,
        serial: u32,
        storage: SecretStorageSpec,
        secret: &SecretMaterial,
        force: bool,
    ) -> Result<()>;
    /// 対象 storage spec の secret を読み出す。
    fn load_secret(
        &mut self,
        serial: u32,
        storage: SecretStorageSpec,
        pin: Option<&SecretMaterial>,
    ) -> Result<SecretMaterial>;
    /// local storage の manifest と全 secret を検証する。
    fn verify_local_storage(&mut self, serial: u32, pin: Option<&SecretMaterial>) -> Result<()>;
}
