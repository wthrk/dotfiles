//! YubiKey bootstrap secret storage のデータモデルと装置抽象。
//!
//! wire format と暗号処理の詳細は他モジュールへ分離し、このファイルは呼び出し境界で
//! 安定させたい型と識別子定義だけを保持する。

use std::collections::BTreeMap;

use anyhow::{Result as AnyhowResult, bail};
use secrecy::{ExposeSecret, SecretBox};
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, IntoEnumIterator};
use zeroize::Zeroizing;

use crate::Result;

pub(crate) const BLOB_MAGIC: &[u8] = b"DOTFILES-YK-SECRET\0";
pub(crate) const BLOB_VERSION: u8 = 1;
pub(crate) const ALGORITHM_AES_256_GCM: u8 = 1;
pub(crate) const NONCE_LEN: usize = 12;
pub(crate) const TAG_LEN: usize = 16;
pub(crate) const CONTENT_KEY_LEN: usize = 32;

/// bootstrap secret 本文を明示的な expose が必要な型で保持する。
pub type SecretBytes = SecretBox<Vec<u8>>;

/// manifest を保存する PIV data object ID。
pub const MANIFEST_OBJECT_ID: u32 = 0x005f_ff16;
/// manifest が dotfiles secret recovery 用であることを示す app id。
pub const MANIFEST_APP: &str = "dotfiles.secret-recovery";
/// bootstrap secret storage 専用の retired key management slot。
pub const KEY_SLOT: &str = "82";

/// YubiKey bootstrap に保存できる secret 名。
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    Display,
    EnumIter,
    EnumString,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum SecretName {
    /// Bitwarden login email。
    BwEmail,
    /// Bitwarden login password。
    BwPassword,
    /// Bitwarden Secrets Manager access token。
    BwsAccessToken,
}

impl SecretName {
    /// bootstrap recovery で扱う secret 名を enum 定義順で列挙する。
    pub fn iter() -> impl Iterator<Item = Self> {
        <Self as IntoEnumIterator>::iter()
    }

    /// binary blob に保存する固定 secret id。
    pub fn secret_id(self) -> u8 {
        match self {
            Self::BwEmail => 1,
            Self::BwPassword => 2,
            Self::BwsAccessToken => 3,
        }
    }

    /// secret ごとに割り当てた PIV data object ID。
    pub fn object_id(self) -> u32 {
        match self {
            Self::BwEmail => 0x005f_ff17,
            Self::BwPassword => 0x005f_ff18,
            Self::BwsAccessToken => 0x005f_ff19,
        }
    }

    /// binary blob の secret id を型付き secret 名へ戻す。
    pub fn from_secret_id(secret_id: u8) -> Result<Self> {
        match secret_id {
            1 => Ok(Self::BwEmail),
            2 => Ok(Self::BwPassword),
            3 => Ok(Self::BwsAccessToken),
            _ => bail!("unknown YubiKey secret id: {secret_id}"),
        }
    }
}

/// PIV data object に保存する manifest。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretManifest {
    /// manifest format version。
    pub version: u8,
    /// この repository の secret storage manifest であることを示す app id。
    pub app: String,
}

impl SecretManifest {
    /// この repository が認識する manifest sentinel。
    pub fn expected() -> Self {
        Self {
            version: 1,
            app: MANIFEST_APP.to_owned(),
        }
    }

    /// 読み出した manifest が現在の storage format と一致することを確認する。
    pub fn validate_expected(&self) -> Result<()> {
        if self != &Self::expected() {
            bail!("YubiKey secret manifest does not match dotfiles secret-recovery format");
        }

        Ok(())
    }
}

/// PIV object に保存する暗号化済み secret blob。
#[derive(Clone, PartialEq, Eq)]
pub struct SecretBlob {
    /// blob に入っている secret 名。
    pub name: SecretName,
    /// AES-256-GCM nonce。
    pub nonce: [u8; NONCE_LEN],
    /// YubiKey public key で wrap した content encryption key。
    pub wrapped_key: Zeroizing<Vec<u8>>,
    /// secret 本文の ciphertext。
    pub ciphertext: Zeroizing<Vec<u8>>,
    /// AES-256-GCM authentication tag。
    pub tag: [u8; TAG_LEN],
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

/// 実機 YubiKey と fake test double に共通する最小操作。
pub trait SecretDevice {
    /// device 固有の serial。AEAD additional data にも含める。
    fn serial(&self) -> u32;
    /// secret storage 用 PIV key が存在するか確認する。
    fn key_exists(&mut self) -> Result<bool>;
    /// secret storage 用 PIV key 生成に必要な device 固有条件を確認する。
    fn check_key_generation_preconditions(&mut self) -> Result<()>;
    /// setup で永続書き込みを始める前に management key 認証可否を確認する。
    fn check_management_auth_preconditions(&mut self) -> Result<()>;
    /// secret storage 用 PIV key を device 内で生成する。
    fn generate_key(&mut self) -> Result<()>;
    /// PIV data object を読み取る。存在しない場合は `None` を返す。
    fn read_object(&mut self, object_id: u32) -> Result<Option<Zeroizing<Vec<u8>>>>;
    /// PIV data object に bytes を保存する。
    fn write_object(&mut self, object_id: u32, value: &[u8]) -> Result<()>;
    /// content encryption key を device の public key で wrap する。
    fn wrap_key(&mut self, key: &[u8]) -> Result<Zeroizing<Vec<u8>>>;
    /// wrapped content encryption key を device の private key operation で unwrap する。
    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Zeroizing<Vec<u8>>>;
}

/// summary に出す確認項目の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CheckStatus {
    /// 確認に成功した状態。
    #[serde(rename = "ok")]
    Ok,
    /// 現在の command では実行しない確認項目。
    #[serde(rename = "skipped")]
    Skipped,
}

/// enroll 系 command の成功 summary。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnrollSummary {
    /// 登録対象 YubiKey の serial。
    pub serial: u32,
    /// 登録した YubiKey の role。
    pub role: YubikeyRole,
    /// 登録中に完了した確認項目。
    pub checks: BTreeMap<CheckName, CheckStatus>,
}

/// YubiKey を primary と spare のどちらとして登録したかを表す role。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum YubikeyRole {
    /// 正本 secret を最初に登録する primary YubiKey。
    Primary,
    /// primary から再暗号化した secret を持つ spare YubiKey。
    Spare,
}

/// verify 系 command の成功 summary。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerifySummary {
    /// 検証対象 YubiKey の serial。
    pub serial: u32,
    /// 実行または省略した確認項目。
    pub checks: BTreeMap<CheckName, CheckStatus>,
}

/// summary JSON の `checks` key として使う閉じた確認項目名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckName {
    /// PIV key と manifest の初期作成。
    Setup,
    /// `bw-email` の保存または復号確認。
    BwEmail,
    /// `bw-password` の保存または復号確認。
    BwPassword,
    /// `bws-access-token` の保存または復号確認。
    BwsAccessToken,
    /// YubiKey local storage 上の 3 secret 復号確認。
    LocalStorage,
    /// Bitwarden Secrets Manager への接続確認。
    Bws,
    /// Bitwarden login secret の妥当性確認。
    BwLogin,
}

/// primary / spare 登録で保存する bootstrap secret 一式。
pub struct BootstrapSecrets {
    /// Bitwarden login email bytes。
    pub bw_email: SecretBytes,
    /// Bitwarden login password bytes。
    pub bw_password: SecretBytes,
    /// Bitwarden Secrets Manager access token bytes。
    pub bws_access_token: SecretBytes,
}

impl BootstrapSecrets {
    /// secret 名に対応する平文 bytes を返す。
    pub fn get(&self, name: SecretName) -> &[u8] {
        match name {
            SecretName::BwEmail => self.bw_email.expose_secret(),
            SecretName::BwPassword => self.bw_password.expose_secret(),
            SecretName::BwsAccessToken => self.bws_access_token.expose_secret(),
        }
    }
}

/// raw bytes を bootstrap secret 用 wrapper に入れる。
pub fn secret_bytes(value: Vec<u8>) -> SecretBytes {
    SecretBox::new(Box::new(value))
}

impl SecretBlob {
    /// secret blob を設計資料の binary wire format に encode する。
    pub fn encode(&self) -> AnyhowResult<Zeroizing<Vec<u8>>> {
        crate::secrets::storage::wire::encode_secret_blob(self)
    }

    /// secret blob を設計資料の binary wire format から decode する。
    pub fn decode(input: &[u8]) -> AnyhowResult<Self> {
        crate::secrets::storage::wire::decode_secret_blob(input)
    }
}

/// blob の入れ替えを検出するため AEAD additional data を構築する。
pub(crate) fn additional_data(serial: u32, name: SecretName) -> Vec<u8> {
    [
        &[BLOB_VERSION, name.secret_id()][..],
        &name.object_id().to_be_bytes(),
        &serial.to_be_bytes(),
    ]
    .concat()
}

/// PIV data object ID を manifest 表示用の 8 桁 hex 文字列にする。
pub(crate) fn format_object_id(object_id: u32) -> String {
    format!("0x{object_id:08X}")
}

/// manifest と全 secret blob の object ID を列挙する。
pub(crate) fn storage_object_ids() -> impl Iterator<Item = u32> {
    std::iter::once(MANIFEST_OBJECT_ID).chain(SecretName::iter().map(SecretName::object_id))
}
