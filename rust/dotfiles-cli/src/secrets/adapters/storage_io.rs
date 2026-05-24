//! YubiKey storage の wire 変換を担う adapter。
//!
//! application 層から JSON parse/serialize と blob decode を分離し、
//! device から取得した bytes と domain 型の境界変換だけを担当する。

use anyhow::Context;

use crate::{
    secrets::domain::{PivObjectId, SecretBlob, SecretManifest, SecretName},
    Result,
};

/// expected manifest を JSON bytes に直列化する。
pub(crate) fn encode_expected_manifest() -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&SecretManifest::expected())?)
}

/// manifest object の JSON bytes を domain model に復元する。
pub(crate) fn decode_manifest(bytes: &[u8]) -> Result<SecretManifest> {
    serde_json::from_slice(bytes).context("failed to parse YubiKey secret manifest")
}

/// secret object の bytes を decode し、要求 secret 名との不一致を拒否する。
pub(crate) fn decode_secret_blob(bytes: &[u8], name: SecretName) -> Result<SecretBlob> {
    let blob = SecretBlob::decode(bytes).with_context(|| format!("failed to decode {}", name))?;
    if blob.name != name {
        anyhow::bail!("YubiKey secret blob name does not match requested {}", name);
    }
    Ok(blob)
}

/// storage manifest を保持する object id を返す。
pub(crate) const fn manifest_object_id() -> PivObjectId {
    PivObjectId::MANIFEST
}
