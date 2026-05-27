use anyhow::Result;
use bincode::config;
use serde::{Deserialize, Serialize};

use crate::secrets::support::aead::{aes_256_gcm_from_key, decrypt_detached, encrypt_detached};
use crate::secrets::support::oaep;
use crate::secrets::support::protection::ProtectedSecret;

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
    pub minimum_plaintext_len: usize,
    pub label: &'a str,
}

#[cfg(feature = "secrets-test-stub")]
pub(crate) struct TestSealRequest<'a> {
    pub secret_id: u8,
    pub nonce: [u8; NONCE_LEN],
    pub wrapped_key: Vec<u8>,
    pub plaintext: &'a [u8],
    pub content_key: &'a ProtectedSecret,
    pub aad: &'a [u8],
    pub minimum_plaintext_len: usize,
    pub label: &'a str,
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

    fn wrapped_key_for_secret_id(input: &[u8], expected_secret_id: u8) -> Result<Vec<u8>> {
        Ok(Self::decode_for_secret_id(input, expected_secret_id)?.wrapped_key)
    }
}

pub(crate) fn unwrap_content_key(decrypted: &[u8], key_len: usize) -> Result<ProtectedSecret> {
    oaep::unwrap_oaep_sha256(decrypted, key_len)
}

pub(crate) fn seal(request: SealRequest<'_>) -> Result<Vec<u8>> {
    request.plaintext.with_secret(|plaintext_bytes| {
        ensure_minimum_plaintext_len(
            plaintext_bytes,
            request.minimum_plaintext_len,
            request.label,
        )?;
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

pub(crate) fn wrapped_key_from_blob(input: &[u8], expected_secret_id: u8) -> Result<Vec<u8>> {
    SealedBlob::wrapped_key_for_secret_id(input, expected_secret_id)
}

#[cfg(feature = "secrets-test-stub")]
pub(crate) fn seal_plaintext_bytes_for_test(request: TestSealRequest<'_>) -> Result<Vec<u8>> {
    ensure_minimum_plaintext_len(
        request.plaintext,
        request.minimum_plaintext_len,
        request.label,
    )?;
    let cipher = request.content_key.with_secret(aes_256_gcm_from_key)?;
    let mut ciphertext = request.plaintext.to_vec();
    let tag = encrypt_detached(&cipher, &request.nonce, request.aad, &mut ciphertext)?;
    SealedBlob {
        version: BLOB_VERSION,
        algorithm: ALGORITHM_AES_256_GCM,
        secret_id: request.secret_id,
        nonce: request.nonce,
        wrapped_key: request.wrapped_key,
        ciphertext,
        tag,
    }
    .encode()
}

pub(crate) fn open(
    input: &[u8],
    expected_secret_id: u8,
    content_key: &ProtectedSecret,
    aad: &[u8],
    minimum_plaintext_len: usize,
    label: &str,
) -> Result<ProtectedSecret> {
    let blob = SealedBlob::decode_for_secret_id(input, expected_secret_id)?;
    let cipher = content_key.with_secret(aes_256_gcm_from_key)?;
    let mut secret = ProtectedSecret::new(blob.ciphertext.len())?;
    secret.with_secret_mut(|out| out.copy_from_slice(&blob.ciphertext));
    secret
        .with_secret_mut(|secret_bytes| {
            decrypt_detached(&cipher, &blob.nonce, aad, secret_bytes, &blob.tag)
        })
        .map_err(|_| anyhow::anyhow!("failed to decrypt payload"))?;
    secret.with_secret(|plaintext| {
        ensure_minimum_plaintext_len(plaintext, minimum_plaintext_len, label)
    })?;
    Ok(secret)
}

fn invalid_blob<T>() -> Result<T> {
    Err(invalid_data("invalid sealed secret blob").into())
}

fn ensure_minimum_plaintext_len(
    plaintext: &[u8],
    minimum_plaintext_len: usize,
    label: &str,
) -> Result<()> {
    if plaintext.len() < minimum_plaintext_len {
        return Err(invalid_data(format!("{label} must not be empty")).into());
    }
    Ok(())
}

fn invalid_data(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())
}
