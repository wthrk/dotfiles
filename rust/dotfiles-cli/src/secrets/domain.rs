//! `dotfiles secrets` の domain 層。
//!
//! PIV object に保存する値、wire format、保存規則を定義する。
//! 端末 I/O、process 保護、実機 YubiKey discovery は外側の責務とする。

mod model;
mod wire;

#[cfg(test)]
pub(crate) use model::MANIFEST_APP;
#[cfg(test)]
pub(crate) use model::{BLOB_MAGIC, TAG_LEN};
pub(crate) use model::{
    CONTENT_KEY_LEN, CheckName, CheckStatus, EnrollSummary, KEY_SLOT, NONCE_LEN, PivObjectId,
    SecretBlob, SecretManifest, SecretName, StorageObjectIds, VerifySummary, YubikeyRole,
};
