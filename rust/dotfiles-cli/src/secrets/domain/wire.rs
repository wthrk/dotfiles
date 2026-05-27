//! YubiKey secret storage で使う JSON wire format codec。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::material::SecretMaterial;

/// manifest JSON wire の pure domain 表現。
#[derive(Serialize, Deserialize)]
pub(crate) struct ManifestWire {
    pub(crate) version: u8,
    pub(crate) app: String,
}

impl ManifestWire {
    /// manifest を固定 key の JSON object に encode する。
    pub(crate) fn encode_json(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).context("failed to serialize manifest JSON")
    }

    /// manifest JSON object を decode する。
    pub(crate) fn decode_json(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).context("failed to decode manifest JSON")
    }
}

/// bootstrap secret JSON wire の pure domain 表現。
#[derive(Deserialize)]
pub(crate) struct BootstrapSecretWire {
    #[serde(rename = "bw-email")]
    pub(crate) bw_email: SensitiveBytes,
    #[serde(rename = "bw-password")]
    pub(crate) bw_password: SensitiveBytes,
    #[serde(rename = "bws-access-token")]
    pub(crate) bws_access_token: SensitiveBytes,
}

impl BootstrapSecretWire {
    /// bootstrap secret JSON object を decode する。
    pub(crate) fn decode_json(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).context("failed to decode bootstrap secret JSON")
    }
}

/// drop 時に内部 bytes を消去する秘密値バッファ。
pub(crate) struct SensitiveBytes(SecretMaterial);

impl SensitiveBytes {
    /// 生 bytes を露出せず `ProtectedSecret` 表現へ移譲する。
    pub(crate) fn into_secret_material(self) -> SecretMaterial {
        self.0
    }
}

impl<'de> Deserialize<'de> for SensitiveBytes {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let protected = SecretMaterial::copy_from_slice(value.as_bytes())
            .map_err(|error| serde::de::Error::custom(error.to_string()))?;
        Ok(Self(protected))
    }
}
