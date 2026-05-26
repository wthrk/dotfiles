//! YubiKey secret blob の binary wire format と manifest の JSON wire format の encode / decode。
//!
//! 設計資料で固定した byte 配置・JSON 契約の互換性を守るため、parser と serializer をこの
//! モジュールへ隔離し、他責務から wire 詳細を切り離す。

use crate::Result;
use aes_gcm::{Aes256Gcm, KeyInit, aead::AeadInPlace};
use anyhow::{Context, bail};
use zeroize::Zeroizing;

use super::model::{
    ALGORITHM_AES_256_GCM, BLOB_MAGIC, BLOB_VERSION, BootstrapSecretDocument, MANIFEST_APP,
    NONCE_LEN, SecretBlob, SecretManifest, SecretName, TAG_LEN,
};

const AEAD_NONCE_LEN: usize = 12;
const AEAD_TAG_LEN: usize = 16;

/// AES-256-GCM content key から cipher を構築する。
pub(crate) fn aes_256_gcm_from_key(key: &[u8]) -> Result<Aes256Gcm> {
    Aes256Gcm::new_from_slice(key).context("invalid AES-256-GCM key length")
}

/// detached tag を返す AES-GCM encrypt を実行する。
pub(crate) fn encrypt_detached(
    cipher: &Aes256Gcm,
    nonce: &[u8],
    additional_data: &[u8],
    buffer: &mut [u8],
) -> Result<[u8; AEAD_TAG_LEN]> {
    if nonce.len() != AEAD_NONCE_LEN {
        bail!("invalid AES-256-GCM nonce length");
    }
    let tag = cipher
        .encrypt_in_place_detached(aes_gcm::Nonce::from_slice(nonce), additional_data, buffer)
        .map_err(|error| anyhow::anyhow!("AES-GCM encrypt failed: {error:?}"))
        .context("failed to encrypt protected payload")?;
    tag.as_slice()
        .try_into()
        .map_err(anyhow::Error::new)
        .context("failed to encode AES-GCM tag")
}

/// detached tag を使う AES-GCM decrypt を実行する。
pub(crate) fn decrypt_detached(
    cipher: &Aes256Gcm,
    nonce: &[u8],
    additional_data: &[u8],
    buffer: &mut [u8],
    tag: &[u8],
) -> Result<()> {
    if nonce.len() != AEAD_NONCE_LEN {
        bail!("invalid AES-256-GCM nonce length");
    }
    if tag.len() != AEAD_TAG_LEN {
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
        .context("failed to decrypt protected payload")
}

/// `SecretManifest` を secret storage 設計で固定した JSON wire format へ serialize する。
///
/// 呼び出し元は serialization error を manifest context と共に扱うこと。
pub(crate) fn encode_manifest(manifest: &SecretManifest) -> Result<Vec<u8>> {
    if manifest.app.contains('"') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "manifest app contains unsupported quote character",
        )
        .into());
    }

    Ok(format!(
        "{{\"version\":{},\"app\":\"{}\"}}",
        manifest.version, manifest.app
    )
    .into_bytes())
}

/// bytes を `SecretManifest` として JSON wire format から deserialize する。
///
/// 呼び出し元は、manifest object が存在しないケースをこの関数の外側で処理すること。
pub(crate) fn decode_manifest(bytes: &[u8]) -> Result<SecretManifest> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("failed to parse YubiKey secret manifest as UTF-8: {error}"),
        )
    })?;

    let compact: String = text
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    let prefix = "{\"version\":";
    let middle = ",\"app\":\"";
    let suffix = "\"}";

    if !compact.starts_with(prefix) || !compact.ends_with(suffix) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "failed to parse YubiKey secret manifest",
        )
        .into());
    }

    let body = &compact[prefix.len()..compact.len() - suffix.len()];
    let (version_text, app) = body.split_once(middle).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "failed to parse YubiKey secret manifest",
        )
    })?;

    let version = version_text.parse::<u8>().map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("failed to parse YubiKey secret manifest version: {error}"),
        )
    })?;

    if app != MANIFEST_APP {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "failed to parse YubiKey secret manifest",
        )
        .into());
    }

    Ok(SecretManifest {
        version,
        app: app.to_owned(),
    })
}

/// stdin JSON document を bootstrap secret document として decode し、field 長を検証する。
pub(crate) fn decode_bootstrap_secret_document(
    bytes: &[u8],
    field_limit: usize,
) -> Result<BootstrapSecretDocument> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("failed to parse bootstrap secret JSON: {error}"),
        )
    })?;

    let bw_email = read_required_string_field(&value, "bw-email", field_limit)?;
    let bw_password = read_required_string_field(&value, "bw-password", field_limit)?;
    let bws_access_token = read_required_string_field(&value, "bws-access-token", field_limit)?;

    Ok(BootstrapSecretDocument {
        bw_email: Zeroizing::new(bw_email),
        bw_password: Zeroizing::new(bw_password),
        bws_access_token: Zeroizing::new(bws_access_token),
    })
}

/// `SecretBlob` を secret storage 設計で固定した binary wire format へ serialize する。
pub(crate) fn encode_secret_blob(blob: &SecretBlob) -> Result<Vec<u8>> {
    let wrapped_key_len = u16::try_from(blob.wrapped_key.len()).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("wrapped YubiKey content key is too large: {error}"),
        )
    })?;
    let ciphertext_len = u32::try_from(blob.ciphertext.len()).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("YubiKey secret ciphertext is too large: {error}"),
        )
    })?;

    let total_len = BLOB_MAGIC.len()
        + 3
        + blob.nonce.len()
        + 2
        + blob.wrapped_key.len()
        + 4
        + blob.ciphertext.len()
        + blob.tag.len();
    let mut encoded = Vec::with_capacity(total_len);
    encoded.extend_from_slice(BLOB_MAGIC);
    encoded.extend_from_slice(&[BLOB_VERSION, blob.name.secret_id(), ALGORITHM_AES_256_GCM]);
    encoded.extend_from_slice(&blob.nonce);
    encoded.extend_from_slice(&wrapped_key_len.to_be_bytes());
    encoded.extend_from_slice(&blob.wrapped_key);
    encoded.extend_from_slice(&ciphertext_len.to_be_bytes());
    encoded.extend_from_slice(&blob.ciphertext);
    encoded.extend_from_slice(&blob.tag);
    Ok(encoded)
}

/// PIV object から読んだ bytes を `SecretBlob` として decode する。
///
/// 入力全体を消費できない場合は invalid blob として失敗する。
pub(crate) fn decode_secret_blob(input: &[u8]) -> Result<SecretBlob> {
    parse_secret_blob(input).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid YubiKey secret blob",
        )
        .into()
    })
}

/// secret storage 設計で固定した binary blob format を parse する。
///
/// `docs/secret-recovery/yubikey-secret-storage-design.md` の byte 配置:
/// magic, version, secret_id, algorithm, 12-byte nonce, u16be wrapped_key length,
/// wrapped_key bytes, u32be ciphertext length, ciphertext bytes, 16-byte tag.
fn parse_secret_blob(input: &[u8]) -> Option<SecretBlob> {
    let mut cursor = 0usize;

    let magic = input.get(..BLOB_MAGIC.len())?;
    if magic != BLOB_MAGIC {
        return None;
    }
    cursor += BLOB_MAGIC.len();

    let version = *input.get(cursor)?;
    if version != BLOB_VERSION {
        return None;
    }
    cursor += 1;

    let name = SecretName::from_secret_id(*input.get(cursor)?).ok()?;
    cursor += 1;

    let algorithm = *input.get(cursor)?;
    if algorithm != ALGORITHM_AES_256_GCM {
        return None;
    }
    cursor += 1;

    let nonce = read_fixed::<NONCE_LEN>(input, &mut cursor)?;

    let wrapped_key_len = usize::from(read_be_u16(input, &mut cursor)?);
    let wrapped_key = read_vec(input, &mut cursor, wrapped_key_len)?;

    let ciphertext_len = usize::try_from(read_be_u32(input, &mut cursor)?).ok()?;
    let ciphertext = read_vec(input, &mut cursor, ciphertext_len)?;

    let tag = read_fixed::<TAG_LEN>(input, &mut cursor)?;

    if cursor != input.len() {
        return None;
    }

    Some(SecretBlob {
        name,
        nonce,
        wrapped_key,
        ciphertext,
        tag,
    })
}

/// 固定長領域を配列として読む。
fn read_fixed<const N: usize>(input: &[u8], cursor: &mut usize) -> Option<[u8; N]> {
    let end = cursor.checked_add(N)?;
    let bytes = input.get(*cursor..end)?;
    let array = <[u8; N]>::try_from(bytes).ok()?;
    *cursor = end;
    Some(array)
}

/// big-endian u16 を読む。
fn read_be_u16(input: &[u8], cursor: &mut usize) -> Option<u16> {
    let bytes = read_fixed::<2>(input, cursor)?;
    Some(u16::from_be_bytes(bytes))
}

/// big-endian u32 を読む。
fn read_be_u32(input: &[u8], cursor: &mut usize) -> Option<u32> {
    let bytes = read_fixed::<4>(input, cursor)?;
    Some(u32::from_be_bytes(bytes))
}

/// 可変長 byte 列を読む。
fn read_vec(input: &[u8], cursor: &mut usize, len: usize) -> Option<Vec<u8>> {
    let end = cursor.checked_add(len)?;
    let bytes = input.get(*cursor..end)?;
    *cursor = end;
    Some(bytes.to_vec())
}

fn read_required_string_field(
    value: &serde_json::Value,
    field: &str,
    field_limit: usize,
) -> Result<String> {
    let field_value = value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("bootstrap secret JSON field `{field}` is missing or not a string"),
            )
        })?;
    if field_value.len() > field_limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("JSON field `{field}` exceeds maximum length"),
        )
        .into());
    }
    Ok(field_value.to_owned())
}
