//! `dotfiles secrets` の domain 層。
//!
//! PIV object に保存する値、wire format、device port、保存規則を定義する。
//! 端末 I/O、process 保護、実機 YubiKey discovery は外側の責務とする。

pub mod model;
mod wire;

pub use model::{
    PivObjectId, SecretBlob, SecretManifest, SecretName, StorageObjectIds, CONTENT_KEY_LEN,
    KEY_SLOT, NONCE_LEN, TAG_LEN,
};
pub(crate) use wire::{decode_manifest, encode_manifest};
