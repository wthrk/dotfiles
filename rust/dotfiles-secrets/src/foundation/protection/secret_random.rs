//! Feature-neutral protected random material and RSA-OAEP wrapping primitive。

use anyhow::Context;
use rand_chacha::ChaCha20Rng;
use rand_core::{OsRng, RngCore, SeedableRng};
use rsa::{Oaep, RsaPublicKey};
use sha2::Sha256;
use zeroize::Zeroize;

use crate::Result;

use super::ProtectedSecret;

pub(crate) fn random_secret(len: usize) -> Result<ProtectedSecret> {
    let mut secret = ProtectedSecret::new(len)?;
    secret
        .with_secret_mut(|bytes| OsRng.try_fill_bytes(bytes))
        .context("failed to obtain OS entropy for protected secret material")?;
    Ok(secret)
}

pub(crate) fn fill_os_random(bytes: &mut [u8], purpose: &'static str) -> Result<()> {
    fill_os_random_with(bytes, purpose, |bytes| OsRng.try_fill_bytes(bytes))
}

fn fill_os_random_with(
    bytes: &mut [u8],
    purpose: &'static str,
    fill: impl FnOnce(&mut [u8]) -> std::result::Result<(), rand_core::Error>,
) -> Result<()> {
    fill(bytes).with_context(|| format!("failed to obtain OS entropy for {purpose}"))
}

pub(crate) fn rsa_oaep_encrypt(public: &RsaPublicKey, key: &ProtectedSecret) -> Result<Vec<u8>> {
    let mut seed = [0_u8; 32];
    fill_os_random(&mut seed, "RSA-OAEP seed")?;
    let mut rng = ChaCha20Rng::from_seed(seed);
    seed.zeroize();
    key.with_secret(|bytes| {
        public
            .encrypt(&mut rng, Oaep::new::<Sha256>(), bytes)
            .context("failed to wrap protected secret with RSA-OAEP")
    })
}

#[cfg(test)]
mod tests {
    use super::fill_os_random_with;

    #[test]
    fn entropy_failure_preserves_the_os_source_error() {
        let mut bytes = [0_u8; 12];
        let error = fill_os_random_with(&mut bytes, "test nonce", |_| {
            Err(rand_core::Error::new(std::io::Error::other(
                "fixture entropy source failed",
            )))
        })
        .expect_err("entropy failure must not be converted to success");
        assert_eq!(
            error.to_string(),
            "failed to obtain OS entropy for test nonce"
        );
        assert!(
            error
                .chain()
                .any(|source| source.to_string().contains("fixture entropy source failed"))
        );
    }
}
