//! `SecretManifest` の JSON serialize/deserialize bridge。
//!
//! manifest の wire format encode/decode を adapter 層に集約し、
//! application 層は port 経由の読み書きのみを担う。

use anyhow::Context;

use crate::{
    secrets::{
        domain::{PivObjectId, SecretManifest},
        ports::SecretDevice,
    },
    Result,
};

/// expected manifest を PIV object へ書き込む。
///
/// manifest は secret blob より先に書き、以後の put/get/verify が storage 所有権を判定する sentinel にする。
pub(crate) fn write_manifest<D: SecretDevice>(device: &mut D) -> Result<()> {
    let mut manifest = serde_json::to_vec(&SecretManifest::expected())?;
    device.write_object(PivObjectId::MANIFEST, &mut manifest)
}

/// PIV object から manifest を読み出して parse する。
///
/// manifest が存在しない YubiKey は secret storage 未初期化として扱う。
pub(crate) fn read_manifest<D: SecretDevice>(device: &mut D) -> Result<SecretManifest> {
    let manifest = device
        .read_object(PivObjectId::MANIFEST)?
        .context("YubiKey secret manifest is missing")?;
    serde_json::from_slice(&manifest).context("failed to parse YubiKey secret manifest")
}
