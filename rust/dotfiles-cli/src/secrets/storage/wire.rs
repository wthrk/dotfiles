//! YubiKey secret blob の binary wire format の encode / decode。
//!
//! 設計資料で固定した byte 配置の互換性を守るため、parser と serializer をこの
//! モジュールへ隔離し、他責務から wire 詳細を切り離す。

use anyhow::{Context, Result};
use nom::{
    Parser,
    bytes::complete::{tag, take},
    combinator::{all_consuming, map_res, verify},
    number::complete::{be_u8, be_u16, be_u32},
};
use zeroize::Zeroizing;

use super::model::{
    ALGORITHM_AES_256_GCM, BLOB_MAGIC, BLOB_VERSION, NONCE_LEN, SecretBlob, SecretName, TAG_LEN,
};

pub(crate) fn encode_secret_blob(blob: &SecretBlob) -> Result<Zeroizing<Vec<u8>>> {
    let wrapped_key_len = u16::try_from(blob.wrapped_key.len())
        .context("wrapped YubiKey content key is too large")?;
    let ciphertext_len =
        u32::try_from(blob.ciphertext.len()).context("YubiKey secret ciphertext is too large")?;

    Ok(Zeroizing::new(
        [
            BLOB_MAGIC,
            &[BLOB_VERSION, blob.name.secret_id(), ALGORITHM_AES_256_GCM],
            &blob.nonce,
            &wrapped_key_len.to_be_bytes(),
            &blob.wrapped_key,
            &ciphertext_len.to_be_bytes(),
            &blob.ciphertext,
            &blob.tag,
        ]
        .concat(),
    ))
}

pub(crate) fn decode_secret_blob(input: &[u8]) -> Result<SecretBlob> {
    all_consuming(parse_secret_blob)
        .parse(input)
        .map(|(_, blob)| blob)
        .map_err(|_| anyhow::anyhow!("invalid YubiKey secret blob"))
}

/// `docs/secret-recovery/yubikey-secret-storage-design.md` の binary blob format:
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
            wrapped_key: Zeroizing::new(wrapped_key.to_vec()),
            ciphertext: Zeroizing::new(ciphertext.to_vec()),
            tag,
        },
    ))
}

/// wire format の固定長領域は、短い入力を parse error として全体 decode 失敗へ寄せる。
fn fixed_bytes<const N: usize>(input: &[u8]) -> nom::IResult<&[u8], [u8; N]> {
    map_res(take(N), <[u8; N]>::try_from).parse(input)
}
