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
        .zip(label_hash.iter())
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

/// `rsa` crate の OAEP unpad は非公開 API なので、YubiKey の raw RSA 出力を復号境界で検証する。
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
