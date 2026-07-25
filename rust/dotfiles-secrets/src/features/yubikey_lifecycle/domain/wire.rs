//! YubiKey secret storage で使う JSON wire format codec。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// manifest JSON wire の pure domain 表現。
#[derive(Serialize, Deserialize)]
pub(crate) struct ManifestWire {
    pub(crate) version: u8,
    pub(crate) app: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) slot_public_key_spki: Option<Vec<u8>>,
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
