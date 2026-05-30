//! YubiKey backend へ application が要求する port 契約。
//!
//! この module は device discovery、PIN 方針、secret storage I/O の capability だけを宣言し、
//! YubiKey crate や PIV backend 型を外側へ露出しない。

use super::super::{
    domain::{
        piv::SecretStorageSpec,
        storage::{
            SecretStorageReadInspection, SecretStorageReadIntent, SecretStorageSetupInspection,
            SecretStorageSetupIntent, SecretStorageSetupProbe, SecretStorageWriteInspection,
            SecretStorageWriteIntent,
        },
    },
    support::protection::ProtectedSecret,
};
use crate::Result;

/// use case が primary 対象の serial を確定する capability 契約。
///
/// caller は利用者指定 serial だけを渡し、device discovery や対話選択の詳細を知らない。
/// implementor は候補列挙・選択・非対話時の拒否を外部 I/O 境界で完了し、storage 操作へ進まない。
#[cfg_attr(test, mockall::automock)]
pub trait DeviceSerialPort {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32>;
}

/// use case が対象 device の PIN 要否を判定する capability 契約。
///
/// caller は解決済み serial だけを渡す。implementor は device API の状態確認を行い、
/// PIN 入力そのものや secret storage の読み書きは実行しない。
#[cfg_attr(test, mockall::automock)]
pub trait DevicePinPolicyPort {
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool>;
}

/// use case が spare 対象の serial を確定する capability 契約。
///
/// caller は spare role の候補指定だけを渡し、primary/spare の domain invariant は domain 側で
/// 検証する。implementor は spare device の選択手段を吸収し、role 関係の業務判断を持たない。
#[cfg_attr(test, mockall::automock)]
pub trait SpareDeviceSerialPort {
    fn resolve_spare_device_serial(&mut self, requested_spare_serial: Option<u32>) -> Result<u32>;
}

/// use case が YubiKey secret storage へ要求する高水準 capability 契約。
///
/// caller は domain が作った inspection/intent を順に適用する。implementor は YubiKey PIV I/O、
/// object 読み書き、保護境界との接続を担い、manifest/storage の業務規則を再定義しない。
#[cfg_attr(test, mockall::automock)]
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
    /// 判定済み intent に従って対象 serial の manifest を確定する。
    fn finalize_secret_storage_setup(
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
        secret: &ProtectedSecret,
    ) -> Result<()>;
    /// 読み出し判定に必要な storage 状態を取得する。
    fn inspect_secret_storage_read(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageReadInspection>;
    /// 判定済み intent に従って対象 storage spec の secret を読み出す。
    #[expect(
        clippy::needless_lifetimes,
        reason = "mockall::automock 展開のため named lifetime が必要"
    )]
    fn load_secret<'a>(
        &mut self,
        serial: u32,
        intent: &SecretStorageReadIntent,
        pin: Option<&'a ProtectedSecret>,
    ) -> Result<ProtectedSecret>;
}
