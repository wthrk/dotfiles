//! 保護済み secret 向けの乱数生成 utility。

use rand::RngCore;
use rand_core::OsRng;
use rsa::{Oaep, RsaPublicKey};
use sha2::Sha256;

use crate::Result;

use super::ProtectedSecret;

/// 指定長のランダム secret を生成する。
pub(crate) fn random_secret(len: usize) -> Result<ProtectedSecret> {
    let mut secret = ProtectedSecret::new(len)?;
    secret.with_secret_mut(|bytes| rand::rng().fill_bytes(bytes));
    Ok(secret)
}

/// `ProtectedSecret` を RSA-OAEP(SHA-256) で暗号化する。
pub(crate) fn rsa_oaep_encrypt(public: &RsaPublicKey, key: &ProtectedSecret) -> Result<Vec<u8>> {
    key.with_secret(|bytes| Ok(public.encrypt(&mut OsRng, Oaep::new::<Sha256>(), bytes)?))
}
