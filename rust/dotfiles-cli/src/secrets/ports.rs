//! `dotfiles secrets` application 層が外部境界へ要求する port 契約。
//!
//! この module は capability 契約と境界データのみを定義し、処理手順や変換規則は持たない。

use super::domain::{
    manifest::BootstrapSecretDocument,
    material::SecretMaterial,
    piv::SecretStorageSpec,
    storage::{
        SecretStorageReadInspection, SecretStorageReadIntent, SecretStorageSetupInspection,
        SecretStorageSetupIntent, SecretStorageSetupProbe, SecretStorageWriteInspection,
        SecretStorageWriteIntent,
    },
    values::{EnrollSummary, VerifySummary},
};
use anyhow::Result;

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
    fn read_bw_email_secret(&self) -> Result<SecretMaterial>;
    fn read_bw_password_secret(&self) -> Result<SecretMaterial>;
    fn read_bws_access_token_secret(&self) -> Result<SecretMaterial>;
    fn read_streamed_secret(&self) -> Result<SecretMaterial>;
}

/// bootstrap secret 文書を取得する capability 契約。
pub trait BootstrapSecretDocumentInputPort {
    fn read_bootstrap_secret_document(&self) -> Result<BootstrapSecretDocument>;
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
    /// setup 判定に必要な storage 状態を取得する。
    fn inspect_secret_storage_setup(
        &mut self,
        serial: u32,
        probe: &SecretStorageSetupProbe,
    ) -> Result<SecretStorageSetupInspection>;
    /// 判定済み intent に従って対象 serial の secret storage を初期化する。
    fn initialize_secret_storage(
        &mut self,
        serial: u32,
        intent: SecretStorageSetupIntent,
    ) -> Result<()>;
    /// 書き込み判定に必要な storage 状態を取得する。
    fn inspect_secret_storage_write(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageWriteInspection>;
    /// 判定済み intent に従って対象 storage spec の secret を保存する。
    fn store_secret(
        &mut self,
        serial: u32,
        intent: SecretStorageWriteIntent,
        secret: &SecretMaterial,
    ) -> Result<()>;
    /// 読み出し判定に必要な storage 状態を取得する。
    fn inspect_secret_storage_read(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageReadInspection>;
    /// 判定済み intent に従って対象 storage spec の secret を読み出す。
    fn load_secret(
        &mut self,
        serial: u32,
        intent: SecretStorageReadIntent,
        pin: Option<&SecretMaterial>,
    ) -> Result<SecretMaterial>;
}
