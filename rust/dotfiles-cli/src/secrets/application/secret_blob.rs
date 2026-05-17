//! YubiKey secret blob の暗号処理境界。
//!
//! content key 生成、AEAD 追加認証データ、YubiKey wrap/unwrap をこのモジュールへ集約し、
//! 操作フロー側から暗号手順の詳細を分離する。

use std::io::Write;

use aes_gcm::{Aes256Gcm, KeyInit, aead::AeadInPlace};
use anyhow::bail;
use rand::Rng;

use crate::Result;
use crate::secrets::support::protection::{ProtectedInputBuffer, ProtectedSecret, SecretSession};

use crate::secrets::domain::{CONTENT_KEY_LEN, NONCE_LEN, SecretBlob, SecretDevice, SecretName};

/// secret 本文を per-secret content key で暗号化し、保存用 blob を構築する。
///
/// content key は device public key で wrap し、AEAD additional data には secret 名由来の
/// 保存 context を使う。
pub(crate) fn encrypt_secret<D: SecretDevice>(
    device: &mut D,
    name: SecretName,
    secret: &[u8],
    session: &SecretSession,
) -> Result<SecretBlob> {
    let mut content_key = ProtectedInputBuffer::new(CONTENT_KEY_LEN, session)?;
    content_key.write_all(&[0; CONTENT_KEY_LEN])?;
    rand::rng().fill(content_key.as_mut_slice());
    let nonce = rand::random::<[u8; NONCE_LEN]>();
    let cipher = Aes256Gcm::new_from_slice(content_key.as_slice())
        .map_err(|_| anyhow::anyhow!("invalid AES-256-GCM key length"))?;
    let mut ciphertext = ProtectedInputBuffer::new(secret.len(), session)?;
    ciphertext.write_all(secret)?;
    let tag = cipher
        .encrypt_in_place_detached(
            aes_gcm::Nonce::from_slice(&nonce),
            &name.additional_data(device.serial()),
            ciphertext.as_mut_slice(),
        )
        .map_err(|_| anyhow::anyhow!("failed to encrypt YubiKey secret"))?;
    let tag = tag
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("failed to encrypt YubiKey secret"))?;

    let wrapped_key = device.wrap_key(content_key.as_slice())?;

    Ok(SecretBlob {
        name,
        nonce,
        wrapped_key,
        ciphertext: ciphertext.as_slice().to_vec(),
        tag,
    })
}

/// 保存用 blob を検証し、secret 本文を保護済み値へ復号する。
///
/// 復号先 allocation は session の memory lock 範囲に含め、平文は `ProtectedSecret` の
/// closure API 以外へ渡さない。
pub(crate) fn decrypt_secret_protected<'session, D: SecretDevice>(
    device: &mut D,
    blob: &SecretBlob,
    session: &'session SecretSession,
) -> Result<ProtectedSecret<'session>> {
    let mut content_key = ProtectedInputBuffer::new(CONTENT_KEY_LEN + 1, session)?;
    device.write_unwrapped_key(&blob.wrapped_key, &mut content_key)?;
    if content_key.as_slice().len() != CONTENT_KEY_LEN {
        bail!("unwrapped YubiKey content key has invalid length");
    }

    let cipher = Aes256Gcm::new_from_slice(content_key.as_slice())
        .map_err(|_| anyhow::anyhow!("invalid AES-256-GCM key length"))?;
    let mut input = ProtectedInputBuffer::new(blob.ciphertext.len(), session)?;
    input.write_all(&blob.ciphertext)?;
    cipher
        .decrypt_in_place_detached(
            aes_gcm::Nonce::from_slice(&blob.nonce),
            &blob.name.additional_data(device.serial()),
            input.as_mut_slice(),
            aes_gcm::Tag::from_slice(&blob.tag),
        )
        .map_err(|_| anyhow::anyhow!("failed to decrypt {}", blob.name))?;
    input.into_protected_secret(session)
}
