//! YubiKey secret blob の binary wire format と manifest の JSON wire format の encode / decode。
//!
//! 設計資料で固定した byte 配置・JSON 契約の互換性を守るため、parser と serializer をこの
//! モジュールへ隔離し、他責務から wire 詳細を切り離す。

use anyhow::{Context, Result};
use nom::{
    bytes::complete::{tag, take},
    combinator::{all_consuming, map_res, verify},
    number::complete::{be_u16, be_u32, be_u8},
    Parser,
};

use super::model::{
    SecretBlob, SecretManifest, SecretName, ALGORITHM_AES_256_GCM, BLOB_MAGIC, BLOB_VERSION,
    NONCE_LEN, TAG_LEN,
};

/// `SecretManifest` を設計資料で固定した JSON wire format へ serialize する。
///
/// 呼び出し元は serialization error を manifest context と共に扱うこと。
pub(crate) fn encode_manifest(manifest: &SecretManifest) -> Result<Vec<u8>> {
    serde_json::to_vec(manifest).context("failed to serialize YubiKey secret manifest")
}

/// bytes を `SecretManifest` として JSON wire format から deserialize する。
///
/// manifest が存在しない場合は `None` を渡し、存在する場合は bytes を渡すこと。
pub(crate) fn decode_manifest(bytes: &[u8]) -> Result<SecretManifest> {
    serde_json::from_slice(bytes).context("failed to parse YubiKey secret manifest")
}

/// `SecretBlob` を設計資料で固定した binary wire format へ serialize する。
pub(crate) fn encode_secret_blob(blob: &SecretBlob) -> Result<Vec<u8>> {
    let wrapped_key_len = u16::try_from(blob.wrapped_key.len())
        .context("wrapped YubiKey content key is too large")?;
    let ciphertext_len =
        u32::try_from(blob.ciphertext.len()).context("YubiKey secret ciphertext is too large")?;

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
    all_consuming(parse_secret_blob)
        .parse(input)
        .map(|(_, blob)| blob)
        .map_err(|_| anyhow::anyhow!("invalid YubiKey secret blob"))
}

/// 設計資料で固定した binary blob format を parse する。
///
/// `docs/secret-recovery/yubikey-secret-storage-design.md` の byte 配置:
/// magic, version, secret_id, algorithm, 12-byte nonce, u16be wrapped_key length,
/// wrapped_key bytes, u32be ciphertext length, ciphertext bytes, 16-byte tag.
fn parse_secret_blob(input: &[u8]) -> nom::IResult<&[u8], SecretBlob> {
    let (input, _) = tag(BLOB_MAGIC).parse(input)?;
    let (input, _) = verify(be_u8, |version| *version == BLOB_VERSION).parse(input)?;
    let (input, name) = map_res(be_u8, SecretName::from_secret_id).parse(input)?;
    let (input, _) = verify(be_u8, |algorithm| *algorithm == ALGORITHM_AES_256_GCM).parse(input)?;
    let (input, nonce) = fixed_bytes::<NONCE_LEN>.parse(input)?;
    let (input, wrapped_key_len) = be_u16(input)?;
    let (input, wrapped_key) = take(wrapped_key_len).parse(input)?;
    let (input, ciphertext_len) = be_u32(input)?;
    let (input, ciphertext) = take(ciphertext_len).parse(input)?;
    let (input, tag) = fixed_bytes::<TAG_LEN>.parse(input)?;

    Ok((
        input,
        SecretBlob {
            name,
            nonce,
            wrapped_key: wrapped_key.to_vec(),
            ciphertext: ciphertext.to_vec(),
            tag,
        },
    ))
}

/// 固定長領域を配列として parse する。
///
/// 必要な byte 数に満たない入力は parse error として全体 decode 失敗へ寄せる。
fn fixed_bytes<const N: usize>(input: &[u8]) -> nom::IResult<&[u8], [u8; N]> {
    map_res(take(N), <[u8; N]>::try_from).parse(input)
}
