//! AES-256-GCM の暗号境界を共通化する utility。
//!
//! key/nonce/tag 長の妥当性確認と detached API 呼び出しをここに集約する。

use aes_gcm::{aead::AeadInPlace, Aes256Gcm, KeyInit};
use anyhow::bail;

use crate::Result;

const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

/// AES-256-GCM content key から cipher を構築する。
pub(crate) fn aes_256_gcm_from_key(key: &[u8]) -> Result<Aes256Gcm> {
    Aes256Gcm::new_from_slice(key).map_err(|_| anyhow::anyhow!("invalid AES-256-GCM key length"))
}

/// detached tag を返す AES-GCM encrypt を実行する。
pub(crate) fn encrypt_detached(
    cipher: &Aes256Gcm,
    nonce: &[u8],
    additional_data: &[u8],
    buffer: &mut [u8],
) -> Result<[u8; TAG_LEN]> {
    if nonce.len() != NONCE_LEN {
        bail!("invalid AES-256-GCM nonce length");
    }
    let tag = cipher
        .encrypt_in_place_detached(aes_gcm::Nonce::from_slice(nonce), additional_data, buffer)
        .map_err(|_| anyhow::anyhow!("failed to encrypt YubiKey secret"))?;
    tag.as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("failed to encrypt YubiKey secret"))
}

/// detached tag を使う AES-GCM decrypt を実行する。
pub(crate) fn decrypt_detached(
    cipher: &Aes256Gcm,
    nonce: &[u8],
    additional_data: &[u8],
    buffer: &mut [u8],
    tag: &[u8],
) -> Result<()> {
    if nonce.len() != NONCE_LEN {
        bail!("invalid AES-256-GCM nonce length");
    }
    if tag.len() != TAG_LEN {
        bail!("invalid AES-256-GCM tag length");
    }
    cipher
        .decrypt_in_place_detached(
            aes_gcm::Nonce::from_slice(nonce),
            additional_data,
            buffer,
            aes_gcm::Tag::from_slice(tag),
        )
        .map_err(|_| anyhow::anyhow!("failed to decrypt YubiKey secret"))
}
