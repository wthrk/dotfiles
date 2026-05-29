//! `dotfiles secrets` application 層が外部境界へ要求する port 契約。
//!
//! この module は capability 契約と境界データのみを定義し、処理手順や変換規則は持たない。

use std::collections::BTreeMap;

use super::domain::{
    piv::SecretStorageSpec,
    storage::{
        SecretStorageReadInspection, SecretStorageReadIntent, SecretStorageSetupInspection,
        SecretStorageSetupIntent, SecretStorageSetupProbe, SecretStorageWriteInspection,
        SecretStorageWriteIntent,
    },
    values::{BwsLookupCandidate, BwsProjectId, BwsSecretId, EnrollSummary, VerifySummary},
};
use super::support::protection::ProtectedSecret;
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

/// use case が PIV PIN を取得するための capability 契約。
///
/// caller は PIN が必要な順序を決めるだけで、端末 echo 制御や buffer 保護を知らない。
/// implementor は入力取得と保護 backend 化を担い、PIN をログ・エラー・表示へ出さない。
#[cfg_attr(test, mockall::automock)]
pub trait PinInputPort {
    fn read_pin(&self) -> Result<ProtectedSecret>;
}

/// use case が必要とする secret 入力 capability 契約。
///
/// caller は必要な secret 種別または stream 入力 capability を明示して呼ぶ。implementor は prompt、
/// stdin、保護 buffer 化を外部 I/O 境界に閉じ、取得した平文を公開 API として返さない。
#[cfg_attr(test, mockall::automock)]
pub trait SecretInputPort {
    fn read_bw_email_secret(&self) -> Result<ProtectedSecret>;
    fn read_bw_password_secret(&self) -> Result<ProtectedSecret>;
    fn read_bws_access_token_secret(&self) -> Result<ProtectedSecret>;
    fn read_streamed_secret(&self) -> Result<ProtectedSecret>;
}

/// use case が対話 rotate の継続可否を外部入力から取得する capability 契約。
///
/// caller は継続確認が必要な地点だけを決める。implementor は TTY 可否と回答取得を扱い、
/// rotate 対象 serial や token 更新の業務判断を持たない。
#[cfg_attr(test, mockall::automock)]
pub trait RotationContinuationPort {
    fn continue_rotation(&self) -> Result<bool>;
}

/// bootstrap secret 文書を取得する capability 契約。
///
/// caller は bootstrap field map を要求するだけで JSON 入力手段や byte 上限を知らない。
/// implementor は入力 decode と secret backend 化を担い、wire/domain の妥当性判断は外へ漏らさない。
#[cfg_attr(test, mockall::automock)]
pub trait BootstrapSecretDocumentInputPort {
    fn read_bootstrap_secret_fields(&self) -> Result<BTreeMap<String, ProtectedSecret>>;
}

/// use case が復号済み secret を出力境界へ渡す契約。
///
/// caller は出力すべき secret material を渡すだけで、端末直書き拒否や stdout 書き込み方式を知らない。
/// implementor は安全な出力先判定を行い、secret を診断文脈へ混ぜない責務を負う。
#[cfg_attr(test, mockall::automock)]
pub trait SecretOutputPort {
    fn write_secret(&self, secret: &ProtectedSecret) -> Result<()>;
}

/// use case が結果報告を出力境界へ渡すための契約。
///
/// caller は domain summary の意味だけを渡す。implementor は JSON key、status 文字列、pretty
/// output など presentation 形式へ翻訳し、summary の成功条件を再定義しない。
#[cfg_attr(test, mockall::automock)]
pub trait ReportPort {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()>;
    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()>;
}

/// use case が Bitwarden Secrets Manager API 境界へ要求する契約。
///
/// caller は domain lookup rule と外部確認 plan を application/domain 側で適用する。implementor は
/// SDK 認証、project/secret 候補の外部 API 取得、ID 境界変換、返却 secret の保護値化だけを担い、
/// 平文 token や secret value を application へ返さない。
#[cfg_attr(test, mockall::automock)]
pub trait BwsClientPort {
    async fn list_bws_projects(
        &self,
        access_token: &ProtectedSecret,
    ) -> Result<Vec<BwsLookupCandidate<BwsProjectId>>>;

    async fn list_bws_secrets(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
    ) -> Result<Vec<BwsLookupCandidate<BwsSecretId>>>;

    async fn fetch_bws_secret_by_id(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> Result<ProtectedSecret>;
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
