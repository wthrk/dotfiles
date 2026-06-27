//! YubiKey backend へ application が要求する port 契約。
//!
//! この module は device discovery、PIN 方針、secret storage I/O の capability だけを宣言し、
//! YubiKey crate や PIV backend 型を外側へ露出しない。

use super::super::{
    domain::{
        gpg_backup::{ConnectedYubiKey, EnvelopeRecipient},
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

/// use case が接続中の単一 YubiKey を確定する capability 契約。
///
/// caller は device discovery の詳細を知らない。implementor は候補列挙と複数接続時の拒否を
/// 外部 I/O 境界で完了し、storage 操作へ進まない。
#[cfg_attr(test, mockall::automock)]
pub trait DeviceSerialPort {
    fn resolve_device_serial(&mut self) -> Result<u32>;
}

/// use case が対象 device の PIN 要否を判定する capability 契約。
///
/// caller は解決済み serial だけを渡す。implementor は device API の状態確認を行い、
/// PIN 入力そのものや secret storage の読み書きは実行しない。
#[cfg_attr(test, mockall::automock)]
pub trait DevicePinPolicyPort {
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool>;
}

/// use case が同一 YubiKey の選択と PIN 方針確認を一体で要求する capability 契約。
///
/// caller は device serial 解決と、その serial に対する PIN 要否確認だけを要求する。implementor は
/// device discovery / 複数接続拒否 / device 状態確認を外部 I/O 境界で完了し、storage 読み書きや use case
/// 手順を隠さない。
#[cfg_attr(test, mockall::automock)]
pub trait YubiKeyDevicePort {
    fn resolve_device_serial(&mut self) -> Result<u32>;
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool>;
}

/// 単一 adapter が両 capability を実装する場合に、統合 capability として合成する blanket impl。
///
/// composition root は serial 解決と PIN 方針を 1 つの device adapter に持たせ、両 capability を要求する
/// use case へ単一 `&mut dyn YubiKeyDevicePort` として渡す。
impl<T> YubiKeyDevicePort for T
where
    T: DeviceSerialPort + DevicePinPolicyPort,
{
    fn resolve_device_serial(&mut self) -> Result<u32> {
        DeviceSerialPort::resolve_device_serial(self)
    }

    fn device_requires_pin(&mut self, serial: u32) -> Result<bool> {
        DevicePinPolicyPort::device_requires_pin(self, serial)
    }
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
    #[expect(
        clippy::needless_lifetimes,
        reason = "mockall::automock 展開のため named lifetime が必要"
    )]
    fn initialize_secret_storage<'a>(
        &mut self,
        serial: u32,
        intent: SecretStorageSetupIntent,
        pin: Option<&'a ProtectedSecret>,
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

/// use case が `gpg-secret-key-backup` recipient 運用のために接続中 YubiKey へ要求する capability 契約。
///
/// caller は recipient 照合・DEK unwrap の順序と停止条件を application/domain 側で決める。
/// implementor は PIV slot `82` 公開鍵 fingerprint の解決、recipient 照合用 identity の構築、device 内 RSA decrypt
/// による DEK unwrap だけを担い、recipient 照合の業務規則そのものは再定義しない。secret key material や
/// DEK は `ProtectedSecret` の借用境界内で扱う。
#[cfg_attr(test, mockall::automock)]
pub trait GpgRecipientPort {
    /// 接続中 YubiKey の PIV slot `82` 公開鍵 fingerprint から、recipient 照合入力を構築する。
    fn resolve_connected_recipient(&mut self, serial: u32) -> Result<ConnectedYubiKey>;

    /// 一致した recipient の `wrapped_dek` を、接続中 YubiKey の PIV slot `82` 秘密鍵で unwrap して DEK を得る。
    #[expect(
        clippy::needless_lifetimes,
        reason = "mockall::automock 展開のため named lifetime が必要"
    )]
    fn unwrap_dek<'a>(
        &mut self,
        serial: u32,
        recipient: &EnvelopeRecipient,
        pin: Option<&'a ProtectedSecret>,
    ) -> Result<ProtectedSecret>;
}
