//! YubiKey backend へ application が要求する port 契約。
//!
//! この module は device discovery と secret storage I/O の capability だけを宣言し、
//! YubiKey crate や PIV backend 型を外側へ露出しない。

use crate::{
    Result,
    features::{
        gpg_backup_recovery::ports::public::{ConnectedYubiKey, EnvelopeRecipient},
        yubikey_lifecycle::domain::{
            piv::{PivDeviceProfile, SecretStorageSpec},
            storage::{
                SecretStorageClearIntent, SecretStorageReadInspection, SecretStorageReadIntent,
                SecretStorageSetupInspection, SecretStorageSetupIntent, SecretStorageSetupProbe,
                SecretStorageStatusInspection, SecretStorageWriteInspection,
                SecretStorageWriteIntent,
            },
        },
    },
    foundation::protection::ProtectedSecret,
};

/// use case が対象 YubiKey の serial を確定する capability 契約。
///
/// caller は利用者指定 serial だけを渡し、device discovery の詳細を知らない。
/// implementor は明示 serial または単一接続 device を解決し、serial 未指定で複数接続された場合は
/// 外部 I/O 境界で拒否して storage 操作へ進まない。
#[cfg_attr(test, mockall::automock)]
pub trait DeviceSerialPort {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32>;
    fn inspect_device_profile(&mut self, serial: u32) -> Result<PivDeviceProfile>;
}

/// use case が YubiKey secret storage へ要求する高水準 capability 契約。
///
/// caller は domain が作った inspection/intent を順に適用する。implementor は YubiKey PIV I/O、
/// object 読み書き、保護境界との接続を担い、manifest/storage の業務規則を再定義しない。
#[cfg_attr(test, mockall::automock)]
pub trait SecretStoragePort {
    /// `setup` と fresh enrollment の PIN 変更前に、current PIN VERIFY と既存 protected
    /// management-key authentication を完了した同じ PIV handle で read-only storage preflight を開始する。
    ///
    /// application は完全な preflight を許可した場合だけ、同じ serial へ `change_piv_pin`、new PIN
    /// による management session 開始を順に要求する。
    fn begin_piv_pin_setup_preflight(
        &mut self,
        serial: u32,
        current_pin: &ProtectedSecret,
    ) -> Result<()>;
    /// `setup` または fresh enrollment の preflight 済み同一 PIV handle で current/new PIN を使い
    /// PIN を変更する。
    ///
    /// existing PIN-protected management key を置換せず、SDK error を分類せず返す。呼び出し後は
    /// new PIN で `begin_piv_management_session` を一回だけ開始する必要がある。
    fn change_piv_pin(
        &mut self,
        serial: u32,
        current_pin: &ProtectedSecret,
        new_pin: &ProtectedSecret,
    ) -> Result<()>;
    /// この process の管理操作に使う設定済み PIV PIN を設定する。
    ///
    /// read/decrypt/status path は呼ばない。application が解決済みの対象 serial とこの保護値を
    /// 渡す。adapter はその serial の fresh PIV handle、または setup/fresh-enrollment preflight が保持する
    /// 同じ handle に一回の `verify_pin`、
    /// PIN-protected management key の取得、認証を完了してから管理操作を行う。
    fn begin_piv_management_session(&mut self, serial: u32, pin: ProtectedSecret) -> Result<()>;
    /// 新たに hidden TTY から読んだ PIN で、次の対象 YubiKey 用の管理 session を開始する。
    ///
    /// 複数 YubiKey を一 command で順番に更新する場合だけ使う。caller は解決済みの次 serial と
    /// 新規 PIN を渡す。前 session の device/PIN は置換時に drop され、別 serial へ再利用
    /// してはならない。
    fn begin_next_piv_management_session(
        &mut self,
        serial: u32,
        pin: ProtectedSecret,
    ) -> Result<()>;
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
    ) -> Result<Vec<u8>>;
    /// 判定済み intent に従って対象 serial の manifest を確定する。
    fn finalize_secret_storage_setup(&mut self, serial: u32, manifest_bytes: Vec<u8>)
    -> Result<()>;
    /// 予約済み slot と custom data object だけを clear する。
    fn clear_secret_storage(
        &mut self,
        serial: u32,
        intent: SecretStorageClearIntent,
    ) -> Result<Vec<u8>>;
    /// 書き込み判定に必要な storage 状態を取得する。
    fn inspect_secret_storage_write(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageWriteInspection>;
    /// read-only `status` 用の PIV PIN / management key を使わない storage 観測。
    fn inspect_secret_storage_status(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageStatusInspection>;
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
    fn load_secret(
        &mut self,
        serial: u32,
        intent: &SecretStorageReadIntent,
    ) -> Result<ProtectedSecret>;
}

/// use case が `gpg-secret-key-backup` recipient 運用のために接続中 YubiKey へ要求する capability 契約。
///
/// caller は recipient 照合・DEK wrap/unwrap の順序と停止条件を application/domain 側で決める。
/// implementor は PIV slot `82` 公開鍵の解決、recipient 照合用 identity の構築、RSA-OAEP-SHA256 での
/// DEK wrap、device 内 RSA decrypt による DEK unwrap だけを担い、recipient 照合の業務規則そのものは
/// 再定義しない。secret key material や DEK は `ProtectedSecret` の借用境界内で扱う。
#[cfg_attr(test, mockall::automock)]
pub trait GpgRecipientPort {
    /// 接続中 YubiKey の serial と PIV slot `82` 公開鍵 fingerprint から、recipient 照合入力を構築する。
    fn resolve_connected_recipient(&mut self, serial: u32) -> Result<ConnectedYubiKey>;

    /// 接続中 YubiKey の PIV slot `82` 公開鍵で DEK を RSA-OAEP-SHA256 wrap し、recipient を構築する。
    ///
    /// backup export（primary 登録）と spare 追加で使い、同一 DEK を recipient 公開鍵で wrap する。
    fn wrap_dek_for_recipient(
        &mut self,
        serial: u32,
        dek: &ProtectedSecret,
    ) -> Result<EnvelopeRecipient>;

    /// 一致した recipient の `wrapped_dek` を、接続中 YubiKey の PIV slot `82` 秘密鍵で unwrap して DEK を得る。
    fn unwrap_dek(&mut self, serial: u32, recipient: &EnvelopeRecipient)
    -> Result<ProtectedSecret>;
}
