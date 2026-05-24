//! YubiKey secret blob の暗号処理を担う support utility。
//!
//! device 操作や wire format を持ち込まず、content key と ciphertext の変換だけを扱う。

use std::io::Write;

use anyhow::bail;
use rand::Rng;

use crate::Result;
use crate::secrets::{
    domain::{CONTENT_KEY_LEN, NONCE_LEN},
    support::{
        aead::{aes_256_gcm_from_key, decrypt_detached, encrypt_detached},
        protection::{ProtectedInputBuffer, ProtectedSecret, SecretSession},
    },
};

/// 平文 secret を指定 additional data で暗号化し、nonce/ciphertext/tag を返す。
pub(crate) fn encrypt_secret_payload(
    secret: &[u8],
    additional_data: &[u8],
    session: &SecretSession,
) -> Result<([u8; NONCE_LEN], Vec<u8>, [u8; 16], Vec<u8>)> {
    let mut content_key = ProtectedInputBuffer::new(CONTENT_KEY_LEN, session)?;
    content_key.write_all(&[0; CONTENT_KEY_LEN])?;
    rand::rng().fill(content_key.as_mut_slice());
    let nonce = rand::random::<[u8; NONCE_LEN]>();
    let cipher = aes_256_gcm_from_key(content_key.as_slice())?;
    let mut ciphertext = ProtectedInputBuffer::new(secret.len(), session)?;
    ciphertext.write_all(secret)?;
    let tag = encrypt_detached(&cipher, &nonce, additional_data, ciphertext.as_mut_slice())?;
    Ok((
        nonce,
        ciphertext.as_slice().to_vec(),
        tag,
        content_key.as_slice().to_vec(),
    ))
}

/// 復号済み content key で blob payload を復号し、保護済み値として返す。
pub(crate) fn decrypt_secret_payload<'session>(
    unwrapped_key: &[u8],
    nonce: &[u8; NONCE_LEN],
    ciphertext: &[u8],
    tag: &[u8; 16],
    additional_data: &[u8],
    session: &'session SecretSession,
) -> Result<ProtectedSecret<'session>> {
    let mut content_key = ProtectedInputBuffer::new(unwrapped_key.len(), session)?;
    content_key.write_all(unwrapped_key)?;
    if content_key.as_slice().len() != CONTENT_KEY_LEN {
        bail!("unwrapped YubiKey content key has invalid length");
    }

    let cipher = aes_256_gcm_from_key(content_key.as_slice())?;
    let mut input = ProtectedInputBuffer::new(ciphertext.len(), session)?;
    input.write_all(ciphertext)?;
    decrypt_detached(&cipher, nonce, additional_data, input.as_mut_slice(), tag)
        .map_err(|_| anyhow::anyhow!("failed to decrypt secret payload"))?;
    input.into_protected_secret(session)
}
