//! `dotfiles secrets` application 層が外部境界へ要求する port 契約。
//!
//! この module は capability 契約のみを定義し、具体的な parser / 暗号処理 / 端末 I/O /
//! device 操作手順は adapter 側の実装へ閉じ込める。

use anyhow::Result;

use super::domain::{
    manifest::BootstrapSecretDocument,
    material::SecretMaterial,
    piv::{PivObjectId, SecretName},
    values::{DeviceCandidate, EnrollSummary, VerifySummary},
};

/// use case が device 候補列挙と open を要求する capability 契約。
pub trait DeviceSelectionPort {
    type Device: SecretDevice;
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>>;
    fn open_device_by_serial(&mut self, serial: u32) -> Result<Self::Device>;
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
    /// primary serial が指定された場合は primary と異なる spare serial を返さなければならない。
    fn resolve_spare_device_serial(
        &mut self,
        primary_serial: Option<u32>,
        requested_spare_serial: Option<u32>,
    ) -> Result<u32>;
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

/// use case が保存済み secret を読み出すための契約。
pub trait SecretLoadPort {
    fn load_secret(
        &mut self,
        serial: u32,
        name: SecretName,
        pin: Option<&SecretMaterial>,
    ) -> Result<SecretMaterial>;
}

/// use case が secret を保存するための契約。
pub trait SecretStorePort {
    fn store_secret(
        &mut self,
        serial: u32,
        name: SecretName,
        force: bool,
        secret: &SecretMaterial,
    ) -> Result<()>;
}

/// use case が YubiKey storage layout を初期化するための契約。
pub trait StorageSetupPort {
    fn setup_storage(&mut self, serial: u32) -> Result<()>;
}

/// use case が local storage の整合性を検証する契約。
pub trait StorageVerifyPort {
    fn verify_local_storage(&mut self, serial: u32, pin: Option<&SecretMaterial>) -> Result<()>;
}

/// use case が結果報告を出力境界へ渡すための契約。
pub trait ReportPort {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()>;
    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()>;
}

/// use case が鍵素材生成に必要な乱数を要求する契約。
pub trait RandomBytesPort {
    fn fill_random_bytes(&self, out: &mut [u8]) -> Result<()>;
}

/// YubiKey device adapter が満たす低水準 device 操作契約。
pub trait SecretDevice {
    /// 接続先 device serial を返す。
    ///
    /// caller はこの値を監査表示・対象識別にのみ使い、device 選択ロジックへ逆流させない。
    fn serial(&self) -> u32;
    /// 管理鍵スロットが初期化済みかを返す。
    ///
    /// implementor は外部 API 差異を吸収し、caller へは bool 契約だけを返す責務を負う。
    fn key_exists(&mut self) -> Result<bool>;
    /// 鍵生成前に必要なデバイス前提条件を検証する。
    ///
    /// caller は `generate_key` の前にこの検証を呼ぶ責務を負う。
    fn check_key_generation_preconditions(&mut self) -> Result<()>;
    /// 既存管理鍵を使う操作の前提条件を検証する。
    ///
    /// caller は write/load 前にこの検証が必要な実装かを考慮する責務を負う。
    fn check_management_auth_preconditions(&mut self) -> Result<()>;
    /// 管理鍵を生成してデバイスへ反映する。
    fn generate_key(&mut self) -> Result<()>;
    /// PIV object bytes を読み出す。
    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>>;
    /// PIV object bytes を書き込む。
    ///
    /// value のゼロ化や一時バッファ管理は implementor 側の責務とする。
    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()>;
    /// content key を device の wrapping key でラップする。
    fn wrap_key(&mut self, key: &SecretMaterial) -> Result<Vec<u8>>;
    /// 現在の device state で PIN 入力が必要かどうかを返す。
    ///
    /// この値は `verify_pin` 呼び出し要否を判断するための signal であり、
    /// PIN 入力手段（prompt / stdin）の選択責務は caller 側にある。
    fn requires_pin_input(&self) -> bool;
    /// PIN 検証を実行し、以後の復号操作に必要な認証状態へ遷移させる。
    ///
    /// 実装は PIN 値を保持し続けず、失敗時は認証状態を進めない責務を負う。
    fn verify_pin(&mut self, pin: &SecretMaterial) -> Result<()>;
    /// `wrap_key` で得た wrapped bytes を復号する。
    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<SecretMaterial>;
}
