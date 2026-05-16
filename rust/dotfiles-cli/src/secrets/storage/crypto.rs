//! YubiKey secret blob の暗号処理境界。
//!
//! content key 生成、AEAD 追加認証データ、YubiKey wrap/unwrap をこのモジュールへ集約し、
//! 操作フロー側から暗号手順の詳細を分離する。

use aes_gcm::{Aes256Gcm, KeyInit, aead::AeadInPlace};
use anyhow::bail;
use zeroize::{Zeroize, Zeroizing};

use crate::Result;

use super::model::{CONTENT_KEY_LEN, NONCE_LEN, SecretBlob, SecretBytes, SecretDevice, SecretName};

/// secret 本文を per-secret content key で暗号化し、保存用 blob を構築する。
///
/// content key は device public key で wrap し、AEAD additional data には secret 名由来の
/// 保存 context を使う。
pub(crate) fn encrypt_secret<D: SecretDevice>(
    device: &mut D,
    name: SecretName,
    secret: &[u8],
) -> Result<SecretBlob> {
    let content_key = Zeroizing::new(rand::random::<[u8; CONTENT_KEY_LEN]>());
    let nonce = rand::random::<[u8; NONCE_LEN]>();
    let cipher = Aes256Gcm::new_from_slice(content_key.as_ref())
        .map_err(|_| anyhow::anyhow!("invalid AES-256-GCM key length"))?;
    let mut ciphertext = Zeroizing::new(secret.to_vec());
    let tag = cipher
        .encrypt_in_place_detached(
            aes_gcm::Nonce::from_slice(&nonce),
            &name.additional_data(device.serial()),
            ciphertext.as_mut(),
        )
        .map_err(|_| anyhow::anyhow!("failed to encrypt YubiKey secret"))?;
    let tag = tag
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("failed to encrypt YubiKey secret"))?;

    let wrapped_key = device.wrap_key(content_key.as_ref())?;

    Ok(SecretBlob {
        name,
        nonce,
        wrapped_key,
        ciphertext,
        tag,
    })
}

/// 保存用 blob を検証し、secret 本文へ復号する。
///
/// content key は device private operation で unwrap し、AEAD tag は secret 名由来の
/// 保存 context で検証する。
pub(crate) fn decrypt_secret<D: SecretDevice>(
    device: &mut D,
    blob: &SecretBlob,
) -> Result<SecretBytes> {
    let mut content_key = device.unwrap_key(&blob.wrapped_key)?;
    if content_key.len() != CONTENT_KEY_LEN {
        bail!("unwrapped YubiKey content key has invalid length");
    }

    let cipher = Aes256Gcm::new_from_slice(&content_key)
        .map_err(|_| anyhow::anyhow!("invalid AES-256-GCM key length"))?;
    let mut plaintext = blob.ciphertext.clone();
    cipher
        .decrypt_in_place_detached(
            aes_gcm::Nonce::from_slice(&blob.nonce),
            &blob.name.additional_data(device.serial()),
            plaintext.as_mut(),
            aes_gcm::Tag::from_slice(&blob.tag),
        )
        .map_err(|_| anyhow::anyhow!("failed to decrypt {}", blob.name))?;
    content_key.zeroize();
    SecretBytes::new_locked(plaintext)
}
