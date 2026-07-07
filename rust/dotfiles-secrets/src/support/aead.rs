//! AES-256-GCM の暗号操作を提供する support utility。

use aes_gcm::{Aes256Gcm, KeyInit, aead::AeadInPlace};
use anyhow::bail;

use crate::Result;

pub(crate) const AES_GCM_NONCE_LEN: usize = 12;

/// 32 byte key material から AES-256-GCM cipher instance を作る。
///
/// caller は key bytes の ownership と保護境界を管理し、この関数には cipher 構築に必要な
/// 一時参照だけを渡す。key 長が AES-256-GCM に適合しない場合は失敗する。
pub(crate) fn aes_256_gcm_from_key(key: &[u8]) -> Result<Aes256Gcm> {
    Aes256Gcm::new_from_slice(key).map_err(anyhow::Error::new)
}

/// `buffer` を AES-256-GCM で in-place 暗号化し、detached tag を返す。
///
/// nonce は 96 bit 固定で、同一 key での nonce 再利用禁止は caller responsibility とする。
/// `additional_data` は認証対象だが暗号化されないため、復号時に同一 bytes を渡す必要がある。
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

/// `buffer` を AES-256-GCM detached tag と AAD で検証して in-place 復号する。
///
/// caller は sealing 時と同じ nonce、AAD、tag を渡す責務を負う。nonce/tag 長の不一致、
/// AAD 不一致、ciphertext 改ざんはいずれも失敗として扱い、認証失敗時の buffer 内容に
/// plaintext としての意味を持たせてはならない。
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
