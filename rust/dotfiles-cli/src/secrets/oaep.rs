//! YubiKey PIV の raw RSA decrypt 結果から RSA-OAEP SHA-256 padding を外す。
//!
//! `yubikey` crate の PIV decrypt は raw RSA 結果を返すため、host 側で OAEP を検証する。

use anyhow::{Context, bail};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::Result;

const OAEP_UNPAD_ERROR: &str = "invalid RSA-OAEP encoded message";

/// YubiKey の raw RSA decrypt 結果から RSA-OAEP SHA-256 padding を検証して外す。
pub(crate) fn oaep_unpad_sha256(encoded: &[u8], key_len: usize) -> Result<Zeroizing<Vec<u8>>> {
    let hash_len = 32;
    if encoded.len() != key_len || key_len < 2 * hash_len + 2 {
        bail!(OAEP_UNPAD_ERROR);
    }

    let (masked_seed, masked_db) = encoded[1..].split_at(hash_len);
    let seed_mask = Zeroizing::new(mgf1_sha256(masked_db, hash_len));
    let seed = Zeroizing::new(
        masked_seed
            .iter()
            .zip(seed_mask.iter())
            .map(|(left, right)| left ^ right)
            .collect::<Vec<u8>>(),
    );
    let db_mask = Zeroizing::new(mgf1_sha256(&seed, key_len - hash_len - 1));
    let db = Zeroizing::new(
        masked_db
            .iter()
            .zip(db_mask.iter())
            .map(|(left, right)| left ^ right)
            .collect::<Vec<u8>>(),
    );

    let label_hash = Sha256::digest([]);
    let label_mismatch = db[..hash_len]
        .iter()
        .zip(label_hash.as_slice())
        .fold(0u8, |acc, (left, right)| acc | (left ^ right));
    let leading_and_label_valid = encoded[0] == 0 && label_mismatch == 0;

    let rest = &db[hash_len..];
    let separator = rest.iter().position(|byte| *byte == 1);
    let padding_valid = separator
        .map(|separator| rest[..separator].iter().all(|byte| *byte == 0))
        .unwrap_or(false);

    if !leading_and_label_valid || !padding_valid {
        bail!(OAEP_UNPAD_ERROR);
    }
    let separator = separator.context(OAEP_UNPAD_ERROR)?;

    Ok(Zeroizing::new(rest[separator + 1..].to_vec()))
}

/// RSA-OAEP SHA-256 で使う MGF1 mask を生成する。
fn mgf1_sha256(seed: &[u8], len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    let mut counter = 0u32;
    while out.len() < len {
        let mut digest = Sha256::new();
        digest.update(seed);
        digest.update(counter.to_be_bytes());
        out.extend_from_slice(&digest.finalize());
        counter += 1;
    }
    out.truncate(len);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oaep_unpad_round_trips_rsa_oaep_sha256() -> Result<()> {
        let message = b"test-content-encryption-key";
        let encoded = oaep_pad_sha256_for_test(message, 256);
        let decoded = oaep_unpad_sha256(&encoded, 256)?;
        assert_eq!(decoded.as_slice(), message);
        Ok(())
    }

    #[test]
    fn oaep_unpad_rejects_invalid_padding() {
        let encoded: Vec<u8> = std::iter::once(1)
            .chain(std::iter::repeat_n(0u8, 255))
            .collect();
        assert!(oaep_unpad_sha256(&encoded, 256).is_err());
    }

    fn oaep_pad_sha256_for_test(message: &[u8], key_len: usize) -> Vec<u8> {
        let hash_len = 32usize;
        let ps_len = key_len - message.len() - (2 * hash_len) - 2;
        let label_hash = Sha256::digest([]);

        let db: Vec<u8> = label_hash
            .as_slice()
            .iter()
            .copied()
            .chain(std::iter::repeat_n(0u8, ps_len))
            .chain(std::iter::once(1))
            .chain(message.iter().copied())
            .collect();

        let seed = [0x42u8; 32];
        let db_mask = mgf1_sha256(&seed, key_len - hash_len - 1);
        let masked_db: Vec<u8> = db
            .iter()
            .zip(db_mask)
            .map(|(left, right)| left ^ right)
            .collect();

        let seed_mask = mgf1_sha256(&masked_db, hash_len);
        let masked_seed: Vec<u8> = seed
            .iter()
            .zip(seed_mask)
            .map(|(left, right)| left ^ right)
            .collect();

        std::iter::once(0)
            .chain(masked_seed)
            .chain(masked_db)
            .collect()
    }
}
