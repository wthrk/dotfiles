//! YubiKey secret blob の暗号化・復号に使う共通暗号処理。
//!
//! この module は wire format や device port を直接扱わず、渡された key/nonce/additional-data と
//! 保護バッファだけで暗号化契約を完結させる。

use std::io::Write;

use anyhow::bail;

use crate::Result;
use crate::secrets::{
    domain::{CONTENT_KEY_LEN, SecretName},
    support::{
        aead::{aes_256_gcm_from_key, decrypt_detached, encrypt_detached},
        protection::{ProtectedInputBuffer, ProtectedSecret, SecretSession},
    },
};

/// 平文 secret を AES-256-GCM で暗号化し、ciphertext と tag を返す。
pub(crate) fn encrypt_secret_payload(
    name: SecretName,
    serial: u32,
    content_key: &[u8],
    nonce: &[u8],
    secret: &[u8],
    session: &SecretSession,
) -> Result<(Vec<u8>, [u8; 16])> {
    let cipher = aes_256_gcm_from_key(content_key)?;
    let mut ciphertext = ProtectedInputBuffer::new(secret.len(), session)?;
    ciphertext.write_all(secret)?;
    let tag = encrypt_detached(
        &cipher,
        nonce,
        &name.additional_data(serial),
        ciphertext.as_mut_slice(),
    )?;
    Ok((ciphertext.as_slice().to_vec(), tag))
}

/// `SecretBlob` の暗号化 payload を復号し、保護済み secret として返す。
pub(crate) fn decrypt_secret_payload<'session>(
    name: SecretName,
    serial: u32,
    content_key: &[u8],
    nonce: &[u8],
    ciphertext: &[u8],
    tag: &[u8; 16],
    session: &'session SecretSession,
) -> Result<ProtectedSecret<'session>> {
    let mut key = ProtectedInputBuffer::new(CONTENT_KEY_LEN + 1, session)?;
    key.write_all(content_key)?;
    if key.as_slice().len() != CONTENT_KEY_LEN {
        bail!("unwrapped YubiKey content key has invalid length");
    }

    let cipher = aes_256_gcm_from_key(key.as_slice())?;
    let mut input = ProtectedInputBuffer::new(ciphertext.len(), session)?;
    input.write_all(ciphertext)?;
    decrypt_detached(
        &cipher,
        nonce,
        &name.additional_data(serial),
        input.as_mut_slice(),
        tag,
    )
    .map_err(|_| anyhow::anyhow!("failed to decrypt {}", name))?;
    input.into_protected_secret(session)
}
