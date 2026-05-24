//! YubiKey bootstrap secret storage のデータモデルと装置抽象。
//!
//! wire format と暗号処理の詳細は他モジュールへ分離し、このファイルは PIV object、
//! secret 名、summary JSON の型付き contract を定義する。

use std::fmt;

use anyhow::{Result as AnyhowResult, bail};
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, IntoEnumIterator};

use crate::Result;

/// secret blob の先頭で dotfiles wire format を識別する magic bytes。
pub(crate) const BLOB_MAGIC: &[u8] = b"DOTFILES-YK-SECRET\0";
/// 現在の binary blob format version。
pub(crate) const BLOB_VERSION: u8 = 1;
/// blob header に保存する AES-256-GCM algorithm id。
pub(crate) const ALGORITHM_AES_256_GCM: u8 = 1;
/// AES-GCM nonce の固定長。
pub(crate) const NONCE_LEN: usize = 12;
/// AES-GCM tag の固定長。
pub(crate) const TAG_LEN: usize = 16;
/// per-secret content encryption key の byte 長。
pub(crate) const CONTENT_KEY_LEN: usize = 32;

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
    /// summary と verify で使う secret 名を安定順で列挙する。
    pub fn iter() -> impl Iterator<Item = Self> {
        <Self as IntoEnumIterator>::iter()
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
    pub wrapped_key: Vec<u8>,
    /// secret 本文の ciphertext。
    pub ciphertext: Vec<u8>,
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

/// YubiKey を primary と spare のどちらとして登録したかを表す role。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum YubikeyRole {
    /// 正本 secret を最初に登録する primary YubiKey。
    Primary,
    /// primary から再暗号化した secret を持つ spare YubiKey。
    Spare,
}

impl SecretBlob {
    /// secret blob を設計資料の binary wire format に encode する。
    pub fn encode(&self) -> AnyhowResult<Vec<u8>> {
        crate::secrets::domain::wire::encode_secret_blob(self)
    }

    /// secret blob を設計資料の binary wire format から decode する。
    pub fn decode(input: &[u8]) -> AnyhowResult<Self> {
        crate::secrets::domain::wire::decode_secret_blob(input)
    }
}
