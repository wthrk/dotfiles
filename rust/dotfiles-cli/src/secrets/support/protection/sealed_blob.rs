//! AEAD payload と wrapped content key を結合する汎用 sealed-blob wire 形式。
//!
//! この module は payload id、nonce、AAD、wrapped key、ciphertext、tag の技術的な
//! 結合だけを扱う。payload id と AAD の値そのものの意味は呼び出し側が決め、この
//! support 境界は与えられた識別子と AAD を AEAD 検証へ渡す責務に限定する。

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

const BLOB_MAGIC: &[u8] = b"PROTECTED-SEALED-BLOB\0";
const BLOB_VERSION: u8 = 1;
const ALGORITHM_AES_256_GCM: u8 = 1;

/// sealing に必要な content key、wrapped key、AAD、nonce を束ねる境界入力。
///
/// caller は payload id と AAD が保護対象 payload の識別規則に一致していること、
/// nonce が同一 content key で再利用されないこと、wrapped key が `content_key` を
/// 復元できる key-wrap 結果であることを保証する。
pub(crate) struct SealRequest<'a> {
    pub payload_id: u8,
    pub nonce: [u8; NONCE_LEN],
    pub wrapped_key: Vec<u8>,
    pub plaintext: &'a ProtectedSecret,
    pub content_key: &'a ProtectedSecret,
    pub aad: &'a [u8],
}

/// content key 生成と key-wrap をこの module 側で行う sealing 境界入力。
///
/// caller は payload id と AAD の意味だけを渡し、content key の生成、nonce 生成、
/// key-wrap callback の適用順序は `seal_with_key_wrap` が一箇所で固定する。
pub(crate) struct SealWithKeyWrapRequest<'a> {
    pub payload_id: u8,
    pub plaintext: &'a ProtectedSecret,
    pub aad: &'a [u8],
}

/// encoded blob 内に保存される AEAD wire record。
///
/// `ciphertext` は `nonce` と caller supplied AAD で AES-256-GCM sealing 済みの bytes、
/// `wrapped_key` は復号時に content key を得るための opaque bytes として扱う。
/// `payload_id` は decode 後に期待値と照合し、別 payload への blob replay を拒否する。
#[derive(Serialize, Deserialize)]
struct SealedBlob {
    version: u8,
    algorithm: u8,
    payload_id: u8,
    nonce: [u8; NONCE_LEN],
    wrapped_key: Vec<u8>,
    ciphertext: Vec<u8>,
    tag: [u8; TAG_LEN],
}

impl SealedBlob {
    fn encode(&self) -> Result<Vec<u8>> {
        let payload = bincode::serde::encode_to_vec(self, config::standard())
            .map_err(|error| invalid_data(format!("failed to encode sealed blob: {error}")))?;
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
            bincode::serde::decode_from_slice::<Self, _>(payload, config::standard())
                .map_err(|error| invalid_data(format!("failed to decode sealed blob: {error}")))?;
        if read != payload.len() {
            return invalid_blob();
        }
        if blob.version != BLOB_VERSION || blob.algorithm != ALGORITHM_AES_256_GCM {
            return invalid_blob();
        }
        Ok(blob)
    }

    fn decode_for_payload_id(input: &[u8], expected_payload_id: u8) -> Result<Self> {
        let blob = Self::decode(input)
            .map_err(|error| anyhow::anyhow!("failed to decode sealed blob: {error}"))?;
        if blob.payload_id != expected_payload_id {
            anyhow::bail!("sealed blob id does not match requested payload id");
        }
        Ok(blob)
    }
}

pub(crate) fn unwrap_content_key(decrypted: &[u8], key_len: usize) -> Result<ProtectedSecret> {
    oaep::unwrap_oaep_sha256(decrypted, key_len)
}

/// 既存 content key と wrapped key を使って plaintext を sealed blob へ変換する。
///
/// plaintext は clone 先の protected buffer 上で in-place 暗号化し、caller supplied AAD
/// は AEAD 認証対象としてだけ使う。payload id は encoded blob に保存され、復号時の
/// replay/swap 検出境界になる。
pub(crate) fn seal(request: SealRequest<'_>) -> Result<Vec<u8>> {
    let cipher = request.content_key.with_secret(aes_256_gcm_from_key)?;
    let mut ciphertext_secret = ProtectedSecret::try_clone(request.plaintext)?;
    let tag = ciphertext_secret.with_secret_mut(|ciphertext_bytes| {
        encrypt_detached(&cipher, &request.nonce, request.aad, ciphertext_bytes)
    })?;
    ciphertext_secret.with_secret(|ciphertext_bytes| {
        SealedBlob {
            version: BLOB_VERSION,
            algorithm: ALGORITHM_AES_256_GCM,
            payload_id: request.payload_id,
            nonce: request.nonce,
            wrapped_key: request.wrapped_key,
            ciphertext: ciphertext_bytes.to_vec(),
            tag,
        }
        .encode()
    })
}

/// 新規 content key と nonce を生成し、caller supplied key-wrap callback 経由で sealing する。
///
/// key-wrap の具体方式は caller 側境界に残し、この関数は生成した protected content key を
/// callback へ渡して得た opaque wrapped key と AEAD ciphertext を同一 blob に束ねる。
pub(crate) fn seal_with_key_wrap(
    request: SealWithKeyWrapRequest<'_>,
    mut wrap_key: impl FnMut(&ProtectedSecret) -> Result<Vec<u8>>,
) -> Result<Vec<u8>> {
    let content_key = secret_random::random_secret(CONTENT_KEY_LEN)?;
    let mut nonce = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut nonce);
    let wrapped_key = wrap_key(&content_key)?;
    seal(SealRequest {
        payload_id: request.payload_id,
        nonce,
        wrapped_key,
        plaintext: request.plaintext,
        content_key: &content_key,
        aad: request.aad,
    })
}

/// encoded blob を payload id と AAD で検証し、key-unwrap callback 経由で plaintext を復元する。
///
/// caller は `expected_payload_id` と AAD を sealing 時と同じ規則で渡す責務を負う。
/// payload id 不一致、AAD/tag 不一致、key unwrap 失敗はいずれも plaintext を返さない。
pub(crate) fn open_with_key_unwrap(
    input: &[u8],
    expected_payload_id: u8,
    mut unwrap_key: impl FnMut(&[u8]) -> Result<ProtectedSecret>,
    aad: &[u8],
) -> Result<ProtectedSecret> {
    let blob = SealedBlob::decode_for_payload_id(input, expected_payload_id)?;
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
    Err(invalid_data("invalid sealed blob").into())
}

fn invalid_data(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    const TEST_PAYLOAD_ID: u8 = 7;
    const TEST_AAD: &[u8] = b"test-aad";

    fn assert_plaintext_bytes_eq(actual: &[u8], expected: &[u8]) {
        let actual_digest: [u8; 32] = Sha256::digest(actual).into();
        let expected_digest: [u8; 32] = Sha256::digest(expected).into();

        assert_eq!(actual.len(), expected.len(), "plaintext length mismatch");
        assert_eq!(actual_digest, expected_digest, "plaintext digest mismatch");
    }

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

    fn encoded_test_blob() -> Result<Vec<u8>> {
        SealedBlob {
            version: BLOB_VERSION,
            algorithm: ALGORITHM_AES_256_GCM,
            payload_id: TEST_PAYLOAD_ID,
            nonce: [7; NONCE_LEN],
            wrapped_key: vec![1, 2, 3],
            ciphertext: vec![4, 5, 6, 7],
            tag: [9; TAG_LEN],
        }
        .encode()
    }

    #[test]
    fn secret_blob_round_trips_binary_format() -> Result<()> {
        let encoded = encoded_test_blob()?;
        let decoded = SealedBlob::decode(&encoded)?;

        assert_eq!(decoded.payload_id, TEST_PAYLOAD_ID);
        assert_eq!(decoded.nonce, [7; NONCE_LEN]);
        assert_eq!(decoded.wrapped_key, vec![1, 2, 3]);
        assert_eq!(decoded.ciphertext, vec![4, 5, 6, 7]);
        assert_eq!(decoded.tag, [9; TAG_LEN]);
        Ok(())
    }

    #[test]
    fn secret_blob_rejects_trailing_bytes() -> Result<()> {
        let mut encoded = encoded_test_blob()?;
        encoded.push(0);

        assert!(SealedBlob::decode(&encoded).is_err());
        Ok(())
    }

    #[test]
    fn secret_blob_rejects_wrapped_key_length_larger_than_payload() -> Result<()> {
        let mut encoded = encoded_test_blob()?;
        encoded.truncate(encoded.len().saturating_sub(TAG_LEN + 1));

        assert!(SealedBlob::decode(&encoded).is_err());
        Ok(())
    }

    #[test]
    fn secret_blob_rejects_wrapped_key_length_smaller_than_payload() -> Result<()> {
        let mut encoded = encoded_test_blob()?;
        let payload = BLOB_MAGIC.len();
        if let Some(byte) = encoded.get_mut(payload) {
            *byte = byte.wrapping_add(1);
        }

        assert!(SealedBlob::decode(&encoded).is_err());
        Ok(())
    }

    #[test]
    fn secret_blob_rejects_ciphertext_length_larger_than_payload() -> Result<()> {
        let mut encoded = encoded_test_blob()?;
        encoded.truncate(encoded.len().saturating_sub(1));

        assert!(SealedBlob::decode(&encoded).is_err());
        Ok(())
    }

    #[test]
    fn secret_blob_rejects_ciphertext_length_smaller_than_payload() -> Result<()> {
        let mut encoded = encoded_test_blob()?;
        encoded.extend_from_slice(b"extra");

        assert!(SealedBlob::decode(&encoded).is_err());
        Ok(())
    }

    #[test]
    fn sealed_blob_round_trips_without_aliasing_plaintext() -> Result<()> {
        let mut plaintext = protected_from_test_bytes(b"secret-value")?;
        let content_key = test_content_key()?;
        let encoded = seal(SealRequest {
            payload_id: TEST_PAYLOAD_ID,
            nonce: [3u8; NONCE_LEN],
            wrapped_key: b"wrapped".to_vec(),
            plaintext: &plaintext,
            content_key: &content_key,
            aad: TEST_AAD,
        })?;

        plaintext.with_secret_mut(|bytes| bytes.fill(b'x'));

        let opened = open_with_key_unwrap(
            &encoded,
            TEST_PAYLOAD_ID,
            |_| ProtectedSecret::try_clone(&content_key),
            TEST_AAD,
        )?;
        opened.with_secret(|bytes| {
            assert_plaintext_bytes_eq(bytes, b"secret-value");
        });
        Ok(())
    }

    #[test]
    fn sealed_blob_rejects_corrupted_input() -> Result<()> {
        let plaintext = protected_from_test_bytes(b"secret-value")?;
        let content_key = test_content_key()?;
        let mut encoded = seal(SealRequest {
            payload_id: TEST_PAYLOAD_ID,
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
            TEST_PAYLOAD_ID,
            |_| ProtectedSecret::try_clone(&content_key),
            TEST_AAD,
        );

        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn sealed_blob_rejects_wrong_payload_id() -> Result<()> {
        let plaintext = protected_from_test_bytes(b"secret-value")?;
        let content_key = test_content_key()?;
        let encoded = seal(SealRequest {
            payload_id: TEST_PAYLOAD_ID,
            nonce: [5u8; NONCE_LEN],
            wrapped_key: b"wrapped".to_vec(),
            plaintext: &plaintext,
            content_key: &content_key,
            aad: TEST_AAD,
        })?;

        let result = open_with_key_unwrap(
            &encoded,
            TEST_PAYLOAD_ID + 1,
            |_| ProtectedSecret::try_clone(&content_key),
            TEST_AAD,
        );

        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn sealed_blob_rejects_wrong_aad() -> Result<()> {
        let plaintext = protected_from_test_bytes(b"secret-value")?;
        let content_key = test_content_key()?;
        let encoded = seal(SealRequest {
            payload_id: TEST_PAYLOAD_ID,
            nonce: [6u8; NONCE_LEN],
            wrapped_key: b"wrapped".to_vec(),
            plaintext: &plaintext,
            content_key: &content_key,
            aad: TEST_AAD,
        })?;

        let result = open_with_key_unwrap(
            &encoded,
            TEST_PAYLOAD_ID,
            |_| ProtectedSecret::try_clone(&content_key),
            b"wrong-aad",
        );

        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn decryption_fails_when_blob_is_replayed_to_different_serial() -> Result<()> {
        let plaintext = protected_from_test_bytes(b"user@example.com")?;
        let content_key = test_content_key()?;
        let encoded = seal(SealRequest {
            payload_id: TEST_PAYLOAD_ID,
            nonce: [8u8; NONCE_LEN],
            wrapped_key: b"wrapped".to_vec(),
            plaintext: &plaintext,
            content_key: &content_key,
            aad: b"serial=1234",
        })?;

        let result = open_with_key_unwrap(
            &encoded,
            TEST_PAYLOAD_ID,
            |_| ProtectedSecret::try_clone(&content_key),
            b"serial=5678",
        );

        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn decryption_fails_when_secret_blob_name_and_object_are_swapped() -> Result<()> {
        let plaintext = protected_from_test_bytes(b"user@example.com")?;
        let content_key = test_content_key()?;
        let encoded = seal(SealRequest {
            payload_id: 1,
            nonce: [9u8; NONCE_LEN],
            wrapped_key: b"wrapped".to_vec(),
            plaintext: &plaintext,
            content_key: &content_key,
            aad: b"object=bw-email",
        })?;

        let result = open_with_key_unwrap(
            &encoded,
            2,
            |_| ProtectedSecret::try_clone(&content_key),
            b"object=bw-password",
        );

        assert!(result.is_err());
        Ok(())
    }
}
