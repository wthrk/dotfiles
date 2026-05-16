//! YubiKey PIV の raw RSA decrypt 結果から RSA-OAEP SHA-256 padding を外す。
//!
//! `yubikey` crate の PIV decrypt は raw RSA 結果を返すため、host 側で OAEP を検証する。

use anyhow::{Context, bail};
use sha2::{Digest, Sha256};

use crate::Result;

const OAEP_UNPAD_ERROR: &str = "invalid RSA-OAEP encoded message";
const HASH_LEN: usize = 32;

/// RSA-OAEP SHA-256 encoded message を検証し、message bytes を返す。
///
/// 入力長が key 長と一致しない場合や padding が不正な場合は同じ error で失敗する。
pub(crate) fn oaep_unpad_sha256(encoded: &[u8], key_len: usize) -> Result<Vec<u8>> {
    if encoded.len() != key_len || key_len < 2 * HASH_LEN + 2 {
        bail!(OAEP_UNPAD_ERROR);
    }

    let (masked_seed, masked_db) = encoded[1..].split_at(HASH_LEN);
    let seed_mask = mgf1_sha256(masked_db, HASH_LEN);
    let seed = masked_seed
        .iter()
        .zip(seed_mask.iter())
        .map(|(left, right)| left ^ right)
        .collect::<Vec<u8>>();
    let db_mask = mgf1_sha256(&seed, key_len - HASH_LEN - 1);
    let db = masked_db
        .iter()
        .zip(db_mask.iter())
        .map(|(left, right)| left ^ right)
        .collect::<Vec<u8>>();

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

    Ok(rest[separator + 1..].to_vec())
}

/// MGF1-SHA256 mask を指定長で生成する。
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

/// OAEP data block から padding separator の位置と padding 妥当性を返す。
///
/// 走査は separator 位置で短絡せず、invalid blob 間の分岐差を狭める。
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

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;
    use rsa::{
        BigUint, Oaep, RsaPrivateKey, RsaPublicKey,
        traits::{PrivateKeyParts, PublicKeyParts},
    };

    #[test]
    fn oaep_unpad_round_trips_rsa_oaep_sha256() -> Result<()> {
        let mut rng = OsRng;
        let private = RsaPrivateKey::new(&mut rng, 2048)?;
        let public = RsaPublicKey::from(&private);
        let message = b"test-content-encryption-key";
        let ciphertext = public.encrypt(&mut rng, Oaep::new::<Sha256>(), message)?;
        let encoded = raw_rsa_decrypt_for_test(&private, &ciphertext)?;
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

    fn raw_rsa_decrypt_for_test(private: &RsaPrivateKey, ciphertext: &[u8]) -> Result<Vec<u8>> {
        let decrypted = BigUint::from_bytes_be(ciphertext).modpow(private.d(), private.n());
        let key_len = private.size();
        let bytes = decrypted.to_bytes_be();
        if bytes.len() > key_len {
            bail!("raw RSA decrypted value is longer than the key size");
        }

        Ok(std::iter::repeat_n(0u8, key_len - bytes.len())
            .chain(bytes)
            .collect())
    }
}
