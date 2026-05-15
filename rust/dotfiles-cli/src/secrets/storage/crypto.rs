//! YubiKey secret blob の暗号処理境界。
//!
//! content key 生成、AEAD 追加認証データ、YubiKey wrap/unwrap をこのモジュールへ集約し、
//! 操作フロー側から暗号手順の詳細を分離する。

use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, AeadInPlace, Payload},
};
use anyhow::{Context, bail};
use zeroize::{Zeroize, Zeroizing};

use crate::Result;

use super::model::{
    CONTENT_KEY_LEN, NONCE_LEN, SecretBlob, SecretDevice, SecretName, TAG_LEN, additional_data,
};
use super::operations::secret_name;

pub(crate) fn encrypt_secret<D: SecretDevice>(
    device: &mut D,
    name: SecretName,
    secret: &[u8],
) -> Result<SecretBlob> {
    let content_key = Zeroizing::new(rand::random::<[u8; CONTENT_KEY_LEN]>());
    let nonce = rand::random::<[u8; NONCE_LEN]>();
    let cipher = Aes256Gcm::new_from_slice(content_key.as_ref())
        .map_err(|_| anyhow::anyhow!("invalid AES-256-GCM key length"))?;
    let ciphertext_and_tag = Zeroizing::new(
        cipher
            .encrypt(
                aes_gcm::Nonce::from_slice(&nonce),
                Payload {
                    msg: secret,
                    aad: &additional_data(device.serial(), name),
                },
            )
            .map_err(|_| anyhow::anyhow!("failed to encrypt YubiKey secret"))?,
    );
    let tag_offset = ciphertext_and_tag
        .len()
        .checked_sub(TAG_LEN)
        .context("AES-256-GCM output is shorter than its tag")?;
    let (ciphertext, tag) = ciphertext_and_tag.split_at(tag_offset);
    let tag = tag
        .try_into()
        .map_err(|_| anyhow::anyhow!("failed to encrypt YubiKey secret"))?;

    let wrapped_key = device.wrap_key(content_key.as_ref())?;

    Ok(SecretBlob {
        name,
        nonce,
        wrapped_key,
        ciphertext: Zeroizing::new(ciphertext.to_vec()),
        tag,
    })
}

pub(crate) fn decrypt_secret<D: SecretDevice>(
    device: &mut D,
    blob: &SecretBlob,
) -> Result<Zeroizing<Vec<u8>>> {
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
            &additional_data(device.serial(), blob.name),
            plaintext.as_mut(),
            aes_gcm::Tag::from_slice(&blob.tag),
        )
        .map_err(|_| anyhow::anyhow!("failed to decrypt {}", secret_name(blob.name)))?;
    content_key.zeroize();
    Ok(plaintext)
}
