//! RSA-OAEP SHA-256 padding を除去する暗号 utility。

use anyhow::{Context, bail};
use sha2::{Digest, Sha256};

use crate::{Result, secrets::domain::material::SecretMaterial};

use super::protection::ProtectedSecret;

const OAEP_UNPAD_ERROR: &str = "invalid RSA-OAEP encoded message";
const HASH_LEN: usize = 32;

/// raw RSA decrypt 出力から OAEP-SHA256 を除去して content key を復元する。
pub(crate) fn unwrap_oaep_sha256(encoded: &[u8], key_len: usize) -> Result<SecretMaterial> {
    if encoded.len() != key_len || key_len < 2 * HASH_LEN + 2 {
        bail!(OAEP_UNPAD_ERROR);
    }

    let (masked_seed, masked_db) = encoded[1..].split_at(HASH_LEN);
    let seed_mask = mgf1_sha256(masked_db, HASH_LEN)?;
    let seed = xor_with_mask(masked_seed, &seed_mask)?;
    let db_mask = seed.with_secret(|seed| mgf1_sha256(seed, key_len - HASH_LEN - 1))?;
    let db = xor_with_mask(masked_db, &db_mask)?;

    db.with_secret(|db| {
        let label_hash = Sha256::digest([]);
        let label_mismatch = db[..HASH_LEN]
            .iter()
            .zip(label_hash.iter())
            .fold(0u8, |acc, (left, right)| acc | (left ^ right));
        let leading_and_label_valid = encoded[0] == 0 && label_mismatch == 0;

        let rest = &db[HASH_LEN..];
        let (separator, padding_valid) = find_oaep_separator(rest);
        if !leading_and_label_valid || !padding_valid {
            bail!(OAEP_UNPAD_ERROR);
        }
        let separator = separator.context(OAEP_UNPAD_ERROR)?;
        SecretMaterial::copy_from_slice(&rest[separator + 1..])
    })
}

fn mgf1_sha256(seed: &[u8], len: usize) -> Result<ProtectedSecret> {
    let mut out = ProtectedSecret::new(len)?;
    out.with_secret_mut(|out_bytes| {
        let mut counter = 0u32;
        let mut written = 0;
        while written < len {
            let mut digest = Sha256::new();
            digest.update(seed);
            digest.update(counter.to_be_bytes());
            let block = digest.finalize();
            let chunk_len = (len - written).min(block.len());
            out_bytes[written..written + chunk_len].copy_from_slice(&block[..chunk_len]);
            written += chunk_len;
            counter += 1;
        }
    });
    Ok(out)
}

fn xor_with_mask(masked: &[u8], mask: &ProtectedSecret) -> Result<ProtectedSecret> {
    debug_assert_eq!(masked.len(), mask.len());
    let mut out = ProtectedSecret::new(masked.len())?;
    out.with_secret_mut(|out_bytes| {
        mask.with_secret(|mask_bytes| {
            for ((dst, left), right) in out_bytes
                .iter_mut()
                .zip(masked.iter())
                .zip(mask_bytes.iter())
            {
                *dst = *left ^ *right;
            }
        });
    });
    Ok(out)
}

fn find_oaep_separator(rest: &[u8]) -> (Option<usize>, bool) {
    let mut separator = None;
    let mut padding_mismatch = 0u8;
    for (index, byte) in rest.iter().copied().enumerate() {
        if separator.is_none() {
            if byte == 1 {
                separator = Some(index);
            } else {
                padding_mismatch |= byte;
            }
        }
    }
    (separator, separator.is_some() && padding_mismatch == 0)
}
