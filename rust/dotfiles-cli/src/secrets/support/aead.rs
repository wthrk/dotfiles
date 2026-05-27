//! AES-256-GCM の暗号操作を提供する support utility。

use aes_gcm::{Aes256Gcm, KeyInit, aead::AeadInPlace};
use anyhow::bail;

use crate::Result;

pub(crate) const AES_GCM_NONCE_LEN: usize = 12;

pub(crate) fn aes_256_gcm_from_key(key: &[u8]) -> Result<Aes256Gcm> {
    Aes256Gcm::new_from_slice(key).map_err(anyhow::Error::new)
}

pub(crate) fn encrypt_detached(
    cipher: &Aes256Gcm,
    nonce: &[u8],
    additional_data: &[u8],
    buffer: &mut [u8],
) -> Result<[u8; 16]> {
    if nonce.len() != AES_GCM_NONCE_LEN {
        bail!("invalid AES-256-GCM nonce length");
    }
    let tag = cipher
        .encrypt_in_place_detached(aes_gcm::Nonce::from_slice(nonce), additional_data, buffer)
        .map_err(|error| anyhow::anyhow!("AES-GCM encrypt failed: {error:?}"))?;
    tag.as_slice().try_into().map_err(anyhow::Error::new)
}

pub(crate) fn decrypt_detached(
    cipher: &Aes256Gcm,
    nonce: &[u8],
    additional_data: &[u8],
    buffer: &mut [u8],
    tag: &[u8],
) -> Result<()> {
    if nonce.len() != AES_GCM_NONCE_LEN {
        bail!("invalid AES-256-GCM nonce length");
    }
    if tag.len() != 16 {
        bail!("invalid AES-GCM tag length");
    }
    cipher
        .decrypt_in_place_detached(
            aes_gcm::Nonce::from_slice(nonce),
            additional_data,
            buffer,
            aes_gcm::Tag::from_slice(tag),
        )
        .map_err(|error| anyhow::anyhow!("AES-GCM decrypt failed: {error:?}"))
}
