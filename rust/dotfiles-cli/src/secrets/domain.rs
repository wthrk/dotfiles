//! `dotfiles secrets` の domain 層。
//!
//! PIV object に保存する値、wire format、device port、保存規則を定義する。
//! 端末 I/O、process 保護、実機 YubiKey discovery は外側の責務とする。

pub mod model;
mod wire;

pub use model::{
    BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT, BootstrapSecretDocument, CONTENT_KEY_LEN, CheckName,
    CheckStatus, EnrollPrimaryCommand, EnrollSpareCommand, EnrollSummary, ExternalCheck,
    GetCommand, KEY_SLOT, NONCE_LEN, PIV_PIN_MAX_LEN, PIV_PIN_MIN_LEN, PivObjectId, PutCommand,
    RotateBwsTokenCommand, SecretBlob, SecretManifest, SecretName, SetupCommand, StorageObjectIds,
    TAG_LEN, VerifySummary, VerifyYubikeyCommand, YubikeyRole, decode_initialized_manifest,
    ensure_secret_value_non_empty, ensure_storage_setup_allowed,
};
pub(crate) use wire::{
    aes_256_gcm_from_key, decode_bootstrap_secret_document, decode_manifest, decrypt_detached,
    encode_manifest, encrypt_detached,
};
