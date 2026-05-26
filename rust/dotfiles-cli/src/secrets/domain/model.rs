//! YubiKey bootstrap secret storage のデータモデル。
//!
//! wire format と暗号処理の詳細は他モジュールへ分離し、このファイルは PIV object、
//! secret 名、use case outcome summary の型付き contract を定義する。

use std::{collections::BTreeMap, fmt, str::FromStr};

use anyhow::Context;
use zeroize::Zeroizing;

use crate::Result;

/// secret blob の先頭で dotfiles wire format を識別する magic bytes。
pub const BLOB_MAGIC: &[u8] = b"DOTFILES-YK-SECRET\0";
/// 現在の binary blob format version。
pub(crate) const BLOB_VERSION: u8 = 1;
/// blob header に保存する AES-256-GCM algorithm id。
pub(crate) const ALGORITHM_AES_256_GCM: u8 = 1;
/// AES-GCM nonce の固定長。
pub const NONCE_LEN: usize = 12;
/// AES-GCM tag の固定長。
pub const TAG_LEN: usize = 16;
/// per-secret content encryption key の byte 長。
pub const CONTENT_KEY_LEN: usize = 32;
/// YubiKey PIV PIN の最小 byte 長。
pub const PIV_PIN_MIN_LEN: usize = 6;
/// YubiKey PIV PIN の最大 byte 長。
pub const PIV_PIN_MAX_LEN: usize = 8;
/// bootstrap secret JSON 各 field に許可する最大 byte 長。
pub const BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT: usize = 16 * 1024;

/// PIV data object ID を型付き値として表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PivObjectId(u32);

impl PivObjectId {
    /// manifest を保存する PIV data object ID。
    pub const MANIFEST: Self = Self(0x005f_ff16);

    /// PIV data object API に渡す raw object ID を返す。
    pub fn value(self) -> u32 {
        self.0
    }

    /// PIV object ID を AEAD additional data 用の big-endian bytes へ変換する。
    fn to_be_bytes(self) -> [u8; 4] {
        self.0.to_be_bytes()
    }
}

impl fmt::Display for PivObjectId {
    /// PIV object ID を 8 桁 hex 表記で表示する。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "0x{:08X}", self.0)
    }
}

/// manifest が dotfiles secret recovery 用であることを示す app id。
pub const MANIFEST_APP: &str = "dotfiles.secret-recovery";
/// bootstrap secret storage 専用の retired key management slot。
pub const KEY_SLOT: &str = "82";

/// YubiKey secret storage が予約する PIV data object ID 集合。
pub struct StorageObjectIds;

impl StorageObjectIds {
    /// manifest と全 secret blob の object ID を列挙する。
    pub fn iter() -> impl Iterator<Item = PivObjectId> {
        std::iter::once(PivObjectId::MANIFEST).chain(SecretName::iter().map(SecretName::object_id))
    }
}

/// YubiKey bootstrap に保存できる secret 名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SecretName {
    /// Bitwarden login email。
    BwEmail,
    /// Bitwarden login password。
    BwPassword,
    /// Bitwarden Secrets Manager access token。
    BwsAccessToken,
}

impl SecretName {
    /// summary と verify で使う secret 名を安定順で列挙する。
    pub fn iter() -> impl Iterator<Item = Self> {
        [Self::BwEmail, Self::BwPassword, Self::BwsAccessToken].into_iter()
    }

    /// binary blob header に保存する固定 secret id を返す。
    pub fn secret_id(self) -> u8 {
        match self {
            Self::BwEmail => 1,
            Self::BwPassword => 2,
            Self::BwsAccessToken => 3,
        }
    }

    /// secret ごとに割り当てた PIV data object ID を返す。
    pub fn object_id(self) -> PivObjectId {
        match self {
            Self::BwEmail => PivObjectId(0x005f_ff17),
            Self::BwPassword => PivObjectId(0x005f_ff18),
            Self::BwsAccessToken => PivObjectId(0x005f_ff19),
        }
    }

    /// 対話入力時に可視入力を許可する secret かどうかを返す。
    pub fn uses_visible_input(self) -> bool {
        matches!(self, Self::BwEmail)
    }

    /// AEAD additional data に使う保存 context bytes を構築する。
    ///
    /// version、secret id、object ID、device serial を含め、blob の差し替えを検出する。
    pub fn additional_data(self, serial: u32) -> Vec<u8> {
        [
            &[BLOB_VERSION, self.secret_id()][..],
            &self.object_id().to_be_bytes(),
            &serial.to_be_bytes(),
        ]
        .concat()
    }

    /// binary blob の secret id を型付き secret 名へ戻す。
    pub fn from_secret_id(secret_id: u8) -> Result<Self> {
        match secret_id {
            1 => Ok(Self::BwEmail),
            2 => Ok(Self::BwPassword),
            3 => Ok(Self::BwsAccessToken),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown YubiKey secret id: {secret_id}"),
            )
            .into()),
        }
    }
}

impl fmt::Display for SecretName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::BwEmail => "bw-email",
            Self::BwPassword => "bw-password",
            Self::BwsAccessToken => "bws-access-token",
        })
    }
}

impl FromStr for SecretName {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "bw-email" => Ok(Self::BwEmail),
            "bw-password" => Ok(Self::BwPassword),
            "bws-access-token" => Ok(Self::BwsAccessToken),
            _ => Err(format!("unsupported YubiKey secret name: {value}")),
        }
    }
}

/// PIV data object に保存する manifest。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretManifest {
    /// manifest format version。
    pub version: u8,
    /// この repository の secret storage manifest であることを示す app id。
    pub app: String,
}

/// bootstrap enrollment で受け取る secret document の pure domain 表現。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapSecretDocument {
    pub bw_email: Zeroizing<String>,
    pub bw_password: Zeroizing<String>,
    pub bws_access_token: Zeroizing<String>,
}

impl SecretManifest {
    /// この repository が認識する manifest sentinel を構築する。
    pub fn expected() -> Self {
        Self {
            version: 1,
            app: MANIFEST_APP.to_owned(),
        }
    }

    /// manifest が現在の storage format と一致することを確認する。
    pub fn validate_expected(&self) -> Result<()> {
        if self != &Self::expected() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "YubiKey secret manifest does not match dotfiles secret-recovery format",
            )
            .into());
        }

        Ok(())
    }
}

impl BootstrapSecretDocument {
    /// 対話入力で取得した bootstrap secret 値群を domain document へ復元する。
    ///
    /// use case から UTF-8 復元手順を排除し、各 field 名に対応する validation error をこの
    /// domain 値の構築責務へ閉じ込める。
    pub fn from_interactive_secrets(
        bw_email: &[u8],
        bw_password: &[u8],
        bws_access_token: &[u8],
    ) -> Result<Self> {
        Ok(Self {
            bw_email: Zeroizing::new(
                String::from_utf8(bw_email.to_vec()).context("bw-email must be valid UTF-8")?,
            ),
            bw_password: Zeroizing::new(
                String::from_utf8(bw_password.to_vec())
                    .context("bw-password must be valid UTF-8")?,
            ),
            bws_access_token: Zeroizing::new(
                String::from_utf8(bws_access_token.to_vec())
                    .context("bws-access-token must be valid UTF-8")?,
            ),
        })
    }
}

/// setup 実行前に storage layout が未初期化状態かを確認する。
///
/// manifest と object 配置の意味規則だけを扱い、実際の device 操作は port 実装側が担う。
pub fn ensure_storage_setup_allowed(
    key_exists: bool,
    manifest_bytes: Option<&[u8]>,
    occupied_object_ids: &[PivObjectId],
) -> Result<()> {
    if key_exists {
        if let Some(manifest_bytes) = manifest_bytes {
            let manifest = crate::secrets::domain::decode_manifest(manifest_bytes)?;
            manifest.validate_expected()?;
            return Err(
                std::io::Error::other("YubiKey secret storage is already initialized").into(),
            );
        }
        return Err(std::io::Error::other("YubiKey PIV slot is already initialized").into());
    }

    if let Some(object_id) = occupied_object_ids.first() {
        return Err(std::io::Error::other(format!(
            "YubiKey PIV object {} already exists",
            object_id
        ))
        .into());
    }

    Ok(())
}

/// manifest object が存在し、期待する storage format と一致することを確認する。
pub fn decode_initialized_manifest(manifest_bytes: Option<&[u8]>) -> Result<SecretManifest> {
    let manifest_bytes = manifest_bytes.context("YubiKey secret manifest is missing")?;
    let manifest = crate::secrets::domain::decode_manifest(manifest_bytes)?;
    manifest.validate_expected()?;
    Ok(manifest)
}

/// 保存または検証対象 secret が空でないことを確認する。
pub fn ensure_secret_value_non_empty(name: SecretName, secret: &[u8]) -> Result<()> {
    if secret.is_empty() {
        return Err(std::io::Error::other(format!("{name} must not be empty")).into());
    }

    Ok(())
}

/// PIV object に保存する暗号化済み secret blob。
#[derive(Clone, PartialEq, Eq)]
pub struct SecretBlob {
    /// blob に入っている secret 名。
    pub name: SecretName,
    /// AES-256-GCM nonce。
    pub nonce: [u8; NONCE_LEN],
    /// YubiKey public key で wrap した content encryption key。
    pub wrapped_key: Vec<u8>,
    /// secret 本文の ciphertext。
    pub ciphertext: Vec<u8>,
    /// AES-256-GCM authentication tag。
    pub tag: [u8; TAG_LEN],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalCheck {
    Bws,
    BwLogin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupCommand {
    pub serial: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PutCommand {
    pub name: SecretName,
    pub serial: Option<u32>,
    pub force: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GetCommand {
    pub name: SecretName,
    pub serial: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnrollPrimaryCommand {
    pub serial: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnrollSpareCommand {
    pub primary_serial: Option<u32>,
    pub spare_serial: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RotateBwsTokenCommand {
    pub serial: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyYubikeyCommand {
    pub serial: Option<u32>,
    pub checks: Vec<ExternalCheck>,
    pub all: bool,
}

impl VerifyYubikeyCommand {
    /// verify use case が要求する対象 serial を返す。
    ///
    /// 現在の verify-yubikey 実行は非対話前提のため、serial 未指定時は command validation として
    /// 停止させる。
    pub fn required_serial(&self) -> Result<u32> {
        self.serial
            .ok_or_else(|| anyhow::anyhow!("pass --serial in non-interactive use"))
    }

    /// verify-yubikey が要求された external check 集合を domain 名へ正規化する。
    ///
    /// `--all` と `--check` の衝突判定、および CLI 入力値から domain check 名への展開を
    /// application から排除する。
    pub fn requested_external_checks(&self) -> Result<Vec<CheckName>> {
        if self.all && !self.checks.is_empty() {
            anyhow::bail!("--all and --check cannot be used together");
        }

        if self.all {
            return Ok(vec![CheckName::Bws, CheckName::BwLogin]);
        }

        Ok(self
            .checks
            .iter()
            .map(|check| match check {
                ExternalCheck::Bws => CheckName::Bws,
                ExternalCheck::BwLogin => CheckName::BwLogin,
            })
            .collect())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Ok,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckName {
    Setup,
    BwEmail,
    BwPassword,
    BwsAccessToken,
    LocalStorage,
    Bws,
    BwLogin,
}

impl CheckName {
    /// CLI / report で使う安定した check 名を返す。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::BwEmail => "bw-email",
            Self::BwPassword => "bw-password",
            Self::BwsAccessToken => "bws-access-token",
            Self::LocalStorage => "local-storage",
            Self::Bws => "bws",
            Self::BwLogin => "bw-login",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YubikeyRole {
    Primary,
    Spare,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrollSummary {
    pub serial: u32,
    pub role: YubikeyRole,
    pub checks: BTreeMap<CheckName, CheckStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifySummary {
    pub serial: u32,
    pub checks: BTreeMap<CheckName, CheckStatus>,
}

impl EnrollSummary {
    /// primary YubiKey enrollment の完了結果を構築する。
    pub fn primary_completed(serial: u32) -> Self {
        Self::completed(serial, YubikeyRole::Primary)
    }

    /// spare YubiKey enrollment の完了結果を構築する。
    pub fn spare_completed(serial: u32) -> Self {
        Self::completed(serial, YubikeyRole::Spare)
    }

    /// enrollment 完了時点の domain summary を構築する。
    ///
    /// report 形式や JSON key は持たず、use case の結果意味だけを保持する。
    pub fn initial(serial: u32, role: YubikeyRole) -> Self {
        Self {
            serial,
            role,
            checks: [
                (CheckName::Setup, CheckStatus::Ok),
                (CheckName::BwEmail, CheckStatus::Ok),
                (CheckName::BwPassword, CheckStatus::Ok),
                (CheckName::BwsAccessToken, CheckStatus::Ok),
                (CheckName::LocalStorage, CheckStatus::Skipped),
            ]
            .into_iter()
            .collect(),
        }
    }

    /// local storage 検証が成功したことを summary へ反映する。
    pub fn mark_local_storage_ok(&mut self) {
        self.checks.insert(CheckName::LocalStorage, CheckStatus::Ok);
    }

    fn completed(serial: u32, role: YubikeyRole) -> Self {
        let mut summary = Self::initial(serial, role);
        summary.mark_local_storage_ok();
        summary
    }
}

impl VerifySummary {
    /// local storage 検証が成功した通常系 summary を構築する。
    pub fn local_storage_verified(serial: u32) -> Self {
        Self::with_local_storage_status(serial, CheckStatus::Ok)
    }

    /// local storage 検証が失敗した停止系 summary を構築する。
    pub fn local_storage_failed(serial: u32) -> Self {
        Self::with_local_storage_status(serial, CheckStatus::Failed)
    }

    /// 未実装の external check を失敗状態として示す summary を構築する。
    pub fn external_checks_unavailable(
        serial: u32,
        checks: impl IntoIterator<Item = CheckName>,
    ) -> Self {
        let mut summary = Self::local_storage_verified(serial);
        for check in checks {
            summary.checks.insert(check, CheckStatus::Failed);
        }
        summary
    }

    fn with_local_storage_status(serial: u32, local_storage: CheckStatus) -> Self {
        Self {
            serial,
            checks: [
                (CheckName::LocalStorage, local_storage),
                (CheckName::Bws, CheckStatus::Skipped),
                (CheckName::BwLogin, CheckStatus::Skipped),
            ]
            .into_iter()
            .collect(),
        }
    }
}

impl std::fmt::Debug for SecretBlob {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SecretBlob")
            .field("name", &self.name)
            .field(
                "nonce",
                &format_args!("<redacted:{} bytes>", self.nonce.len()),
            )
            .field(
                "wrapped_key",
                &format_args!("<redacted:{} bytes>", self.wrapped_key.len()),
            )
            .field(
                "ciphertext",
                &format_args!("<redacted:{} bytes>", self.ciphertext.len()),
            )
            .field("tag", &format_args!("<redacted:{} bytes>", self.tag.len()))
            .finish()
    }
}

impl SecretBlob {
    /// secret blob を設計資料の binary wire format に encode する。
    pub fn encode(&self) -> Result<Vec<u8>> {
        crate::secrets::domain::wire::encode_secret_blob(self)
    }

    /// secret blob を設計資料の binary wire format から decode する。
    pub fn decode(input: &[u8]) -> Result<Self> {
        crate::secrets::domain::wire::decode_secret_blob(input)
    }
}
