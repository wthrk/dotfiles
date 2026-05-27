use anyhow::Result;
use bincode::config;
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::secrets::support::aead::{aes_256_gcm_from_key, decrypt_detached, encrypt_detached};
use crate::secrets::support::protection::{ProtectedSecret, secret_random};

use super::oaep;

pub(crate) const NONCE_LEN: usize = 12;
pub(crate) const TAG_LEN: usize = 16;
pub(crate) const CONTENT_KEY_LEN: usize = 32;

const BLOB_MAGIC: &[u8] = b"DOTFILES-SEALED-SECRET\0";
const BLOB_VERSION: u8 = 1;
const ALGORITHM_AES_256_GCM: u8 = 1;

pub(crate) struct SealRequest<'a> {
    pub secret_id: u8,
    pub nonce: [u8; NONCE_LEN],
    pub wrapped_key: Vec<u8>,
    pub plaintext: &'a ProtectedSecret,
    pub content_key: &'a ProtectedSecret,
    pub aad: &'a [u8],
}

pub(crate) struct SealWithKeyWrapRequest<'a> {
    pub secret_id: u8,
    pub plaintext: &'a ProtectedSecret,
    pub aad: &'a [u8],
}

#[derive(Serialize, Deserialize)]
struct SealedBlob {
    version: u8,
    algorithm: u8,
    secret_id: u8,
    nonce: [u8; NONCE_LEN],
    wrapped_key: Vec<u8>,
    ciphertext: Vec<u8>,
    tag: [u8; TAG_LEN],
}

impl SealedBlob {
    fn encode(&self) -> Result<Vec<u8>> {
        let payload = bincode::serde::encode_to_vec(self, config::standard()).map_err(|error| {
            invalid_data(format!("failed to encode sealed secret blob: {error}"))
        })?;
        let mut encoded = Vec::with_capacity(BLOB_MAGIC.len() + payload.len());
        encoded.extend_from_slice(BLOB_MAGIC);
        encoded.extend_from_slice(&payload);
        Ok(encoded)
    }

    fn decode(input: &[u8]) -> Result<Self> {
        if !input.starts_with(BLOB_MAGIC) {
            return invalid_blob();
        }
        let payload = &input[BLOB_MAGIC.len()..];
        let (blob, read) =
            bincode::serde::decode_from_slice::<Self, _>(payload, config::standard()).map_err(
                |error| invalid_data(format!("failed to decode sealed secret blob: {error}")),
            )?;
        if read != payload.len() {
            return invalid_blob();
        }
        if blob.version != BLOB_VERSION || blob.algorithm != ALGORITHM_AES_256_GCM {
            return invalid_blob();
        }
        Ok(blob)
    }

    fn decode_for_secret_id(input: &[u8], expected_secret_id: u8) -> Result<Self> {
        let blob = Self::decode(input)
            .map_err(|error| anyhow::anyhow!("failed to decode secret blob: {error}"))?;
        if blob.secret_id != expected_secret_id {
            anyhow::bail!("sealed secret blob id does not match requested secret id");
        }
        Ok(blob)
    }
}

pub(crate) fn unwrap_content_key(decrypted: &[u8], key_len: usize) -> Result<ProtectedSecret> {
    oaep::unwrap_oaep_sha256(decrypted, key_len)
}

pub(crate) fn seal(request: SealRequest<'_>) -> Result<Vec<u8>> {
    request.plaintext.with_secret(|_| {
        let cipher = request.content_key.with_secret(aes_256_gcm_from_key)?;
        let mut ciphertext_secret = ProtectedSecret::try_clone(request.plaintext)?;
        let tag = ciphertext_secret.with_secret_mut(|ciphertext_bytes| {
            encrypt_detached(&cipher, &request.nonce, request.aad, ciphertext_bytes)
        })?;
        ciphertext_secret.with_secret(|ciphertext_bytes| {
            SealedBlob {
                version: BLOB_VERSION,
                algorithm: ALGORITHM_AES_256_GCM,
                secret_id: request.secret_id,
                nonce: request.nonce,
                wrapped_key: request.wrapped_key,
                ciphertext: ciphertext_bytes.to_vec(),
                tag,
            }
            .encode()
        })
    })
}

pub(crate) fn seal_with_key_wrap(
    request: SealWithKeyWrapRequest<'_>,
    mut wrap_key: impl FnMut(&ProtectedSecret) -> Result<Vec<u8>>,
) -> Result<Vec<u8>> {
    let content_key = secret_random::random_secret(CONTENT_KEY_LEN)?;
    let mut nonce = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut nonce);
    let wrapped_key = wrap_key(&content_key)?;
    seal(SealRequest {
        secret_id: request.secret_id,
        nonce,
        wrapped_key,
        plaintext: request.plaintext,
        content_key: &content_key,
        aad: request.aad,
    })
}

pub(crate) fn open_with_key_unwrap(
    input: &[u8],
    expected_secret_id: u8,
    mut unwrap_key: impl FnMut(&[u8]) -> Result<ProtectedSecret>,
    aad: &[u8],
) -> Result<ProtectedSecret> {
    let blob = SealedBlob::decode_for_secret_id(input, expected_secret_id)?;
    let content_key = unwrap_key(&blob.wrapped_key)?;
    open_decoded(blob, &content_key, aad)
}

fn open_decoded(
    blob: SealedBlob,
    content_key: &ProtectedSecret,
    aad: &[u8],
) -> Result<ProtectedSecret> {
    let cipher = content_key.with_secret(aes_256_gcm_from_key)?;
    let mut secret = ProtectedSecret::new(blob.ciphertext.len())?;
    secret.with_secret_mut(|out| out.copy_from_slice(&blob.ciphertext));
    secret
        .with_secret_mut(|secret_bytes| {
            decrypt_detached(&cipher, &blob.nonce, aad, secret_bytes, &blob.tag)
        })
        .map_err(|_| anyhow::anyhow!("failed to decrypt payload"))?;
    Ok(secret)
}

fn invalid_blob<T>() -> Result<T> {
    Err(invalid_data("invalid sealed secret blob").into())
}

fn invalid_data(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET_ID: u8 = 7;
    const TEST_AAD: &[u8] = b"test-aad";

    fn protected_from_test_bytes(bytes: &[u8]) -> Result<ProtectedSecret> {
        let mut secret = ProtectedSecret::new(bytes.len())?;
        secret.with_secret_mut(|out| out.copy_from_slice(bytes));
        Ok(secret)
    }

    fn test_content_key() -> Result<ProtectedSecret> {
        let mut key = ProtectedSecret::new(CONTENT_KEY_LEN)?;
        key.with_secret_mut(|bytes| {
            for (index, byte) in bytes.iter_mut().enumerate() {
                *byte = index as u8;
            }
        });
        Ok(key)
    }

    #[test]
    fn sealed_blob_round_trips_without_aliasing_plaintext() -> Result<()> {
        let mut plaintext = protected_from_test_bytes(b"secret-value")?;
        let content_key = test_content_key()?;
        let encoded = seal(SealRequest {
            secret_id: TEST_SECRET_ID,
            nonce: [3u8; NONCE_LEN],
            wrapped_key: b"wrapped".to_vec(),
            plaintext: &plaintext,
            content_key: &content_key,
            aad: TEST_AAD,
        })?;

        plaintext.with_secret_mut(|bytes| bytes.fill(b'x'));

        let opened = open_with_key_unwrap(
            &encoded,
            TEST_SECRET_ID,
            |_| ProtectedSecret::try_clone(&content_key),
            TEST_AAD,
        )?;
        opened.with_secret(|bytes| {
            assert_eq!(bytes, b"secret-value");
        });
        Ok(())
    }

    #[test]
    fn sealed_blob_rejects_corrupted_input() -> Result<()> {
        let plaintext = protected_from_test_bytes(b"secret-value")?;
        let content_key = test_content_key()?;
        let mut encoded = seal(SealRequest {
            secret_id: TEST_SECRET_ID,
            nonce: [4u8; NONCE_LEN],
            wrapped_key: b"wrapped".to_vec(),
            plaintext: &plaintext,
            content_key: &content_key,
            aad: TEST_AAD,
        })?;
        if let Some(last) = encoded.last_mut() {
            *last ^= 0x01;
        } else {
            anyhow::bail!("sealed blob test fixture must not be empty");
        }

        let result = open_with_key_unwrap(
            &encoded,
            TEST_SECRET_ID,
            |_| ProtectedSecret::try_clone(&content_key),
            TEST_AAD,
        );

        assert!(result.is_err());
        Ok(())
    }
}
