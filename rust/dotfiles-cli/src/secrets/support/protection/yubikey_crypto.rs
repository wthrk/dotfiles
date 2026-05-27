use anyhow::Result;
use bincode::config;
use serde::{Deserialize, Serialize};
use yubikey::YubiKey;

use crate::secrets::support::aead::{aes_256_gcm_from_key, decrypt_detached, encrypt_detached};
use crate::secrets::support::oaep;
use crate::secrets::support::protection::ProtectedSecret;

pub(crate) const NONCE_LEN: usize = 12;
pub(crate) const TAG_LEN: usize = 16;
pub(crate) const CONTENT_KEY_LEN: usize = 32;

const BLOB_MAGIC: &[u8] = b"DOTFILES-YK-SECRET\0";
const BLOB_VERSION: u8 = 1;
const ALGORITHM_AES_256_GCM: u8 = 1;
#[cfg(feature = "secrets-test-stub")]
const STUB_WRAP_PREFIX: &[u8] = b"dotfiles-stub-wrapped-v1:";

#[derive(Serialize, Deserialize)]
struct YubikeyBlob {
    version: u8,
    algorithm: u8,
    secret_id: u8,
    nonce: [u8; NONCE_LEN],
    wrapped_key: Vec<u8>,
    ciphertext: Vec<u8>,
    tag: [u8; TAG_LEN],
}

impl YubikeyBlob {
    fn encode(&self) -> Result<Vec<u8>> {
        let payload = bincode::serde::encode_to_vec(self, config::standard()).map_err(|error| {
            invalid_data(format!("failed to encode YubiKey secret blob: {error}"))
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
                |error| invalid_data(format!("failed to decode YubiKey secret blob: {error}")),
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
            anyhow::bail!("YubiKey secret blob name does not match requested secret id");
        }
        Ok(blob)
    }

    fn wrapped_key_for_secret_id(input: &[u8], expected_secret_id: u8) -> Result<Vec<u8>> {
        Ok(Self::decode_for_secret_id(input, expected_secret_id)?.wrapped_key)
    }
}

/// protected PIN を YubiKey SDK へ渡して検証する。
pub(crate) fn verify_pin(yubikey: &mut YubiKey, pin: &ProtectedSecret) -> Result<()> {
    pin.with_secret(|pin_bytes| yubikey.verify_pin(pin_bytes).map_err(anyhow::Error::new))
}

/// YubiKey PIV の raw RSA decrypt 出力から content key を復元する。
pub(crate) fn unwrap_content_key(decrypted: &[u8], key_len: usize) -> Result<ProtectedSecret> {
    oaep::unwrap_oaep_sha256(decrypted, key_len)
}

pub(crate) fn seal_for_storage(
    secret_id: u8,
    nonce: [u8; NONCE_LEN],
    wrapped_key: Vec<u8>,
    plaintext: &ProtectedSecret,
    content_key: &ProtectedSecret,
    aad: &[u8],
    validate_plaintext: impl FnOnce(&[u8]) -> Result<()>,
) -> Result<Vec<u8>> {
    plaintext.with_secret(|plaintext_bytes| {
        validate_plaintext(plaintext_bytes)?;
        let cipher = content_key.with_secret(aes_256_gcm_from_key)?;
        let mut ciphertext_secret = ProtectedSecret::try_clone(plaintext)?;
        let tag = ciphertext_secret.with_secret_mut(|ciphertext_bytes| {
            encrypt_detached(&cipher, &nonce, aad, ciphertext_bytes)
        })?;
        ciphertext_secret.with_secret(|ciphertext_bytes| {
            let blob = YubikeyBlob {
                version: BLOB_VERSION,
                algorithm: ALGORITHM_AES_256_GCM,
                secret_id,
                nonce,
                wrapped_key,
                ciphertext: ciphertext_bytes.to_vec(),
                tag,
            };
            blob.encode()
        })
    })
}

pub(crate) fn wrapped_key_from_blob(input: &[u8], expected_secret_id: u8) -> Result<Vec<u8>> {
    YubikeyBlob::wrapped_key_for_secret_id(input, expected_secret_id)
}

#[cfg(feature = "secrets-test-stub")]
pub(crate) fn stub_wrap_content_key(key: &ProtectedSecret) -> Vec<u8> {
    key.with_secret(|bytes| {
        let mut wrapped = Vec::with_capacity(STUB_WRAP_PREFIX.len() + bytes.len());
        wrapped.extend_from_slice(STUB_WRAP_PREFIX);
        wrapped.extend(bytes.iter().map(|byte| byte ^ 0xa5));
        wrapped
    })
}

#[cfg(feature = "secrets-test-stub")]
pub(crate) fn stub_unwrap_content_key(wrapped_key: &[u8]) -> Result<ProtectedSecret> {
    let Some(masked) = wrapped_key.strip_prefix(STUB_WRAP_PREFIX) else {
        anyhow::bail!("invalid stub-wrapped content key");
    };
    let mut key = ProtectedSecret::new(masked.len())?;
    key.with_secret_mut(|bytes| {
        for (dst, source) in bytes.iter_mut().zip(masked.iter()) {
            *dst = *source ^ 0xa5;
        }
    });
    Ok(key)
}

#[cfg(feature = "secrets-test-stub")]
pub(crate) fn zero_content_key() -> Result<ProtectedSecret> {
    ProtectedSecret::new(CONTENT_KEY_LEN)
}

#[cfg(feature = "secrets-test-stub")]
pub(crate) fn seal_plaintext_bytes_for_test_storage(
    secret_id: u8,
    nonce: [u8; NONCE_LEN],
    wrapped_key: Vec<u8>,
    plaintext: &[u8],
    content_key: &ProtectedSecret,
    aad: &[u8],
    validate_plaintext: impl FnOnce(&[u8]) -> Result<()>,
) -> Result<Vec<u8>> {
    validate_plaintext(plaintext)?;
    let cipher = content_key.with_secret(aes_256_gcm_from_key)?;
    let mut ciphertext = plaintext.to_vec();
    let tag = encrypt_detached(&cipher, &nonce, aad, &mut ciphertext)?;
    YubikeyBlob {
        version: BLOB_VERSION,
        algorithm: ALGORITHM_AES_256_GCM,
        secret_id,
        nonce,
        wrapped_key,
        ciphertext,
        tag,
    }
    .encode()
}

pub(crate) fn open_from_storage(
    input: &[u8],
    expected_secret_id: u8,
    content_key: &ProtectedSecret,
    aad: &[u8],
    validate_plaintext: impl FnOnce(&[u8]) -> Result<()>,
) -> Result<ProtectedSecret> {
    let blob = YubikeyBlob::decode_for_secret_id(input, expected_secret_id)?;
    let cipher = content_key.with_secret(aes_256_gcm_from_key)?;
    let mut secret = ProtectedSecret::new(blob.ciphertext.len())?;
    secret.with_secret_mut(|out| out.copy_from_slice(&blob.ciphertext));
    secret
        .with_secret_mut(|secret_bytes| {
            decrypt_detached(&cipher, &blob.nonce, aad, secret_bytes, &blob.tag)
        })
        .map_err(|_| anyhow::anyhow!("failed to decrypt payload"))?;
    secret.with_secret(validate_plaintext)?;
    Ok(secret)
}

fn invalid_blob<T>() -> Result<T> {
    Err(invalid_data("invalid YubiKey secret blob").into())
}

fn invalid_data(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())
}
