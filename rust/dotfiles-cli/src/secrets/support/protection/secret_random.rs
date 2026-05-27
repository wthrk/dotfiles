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

/// `ProtectedSecret` を RSA-OAEP(SHA-256) で不透明な wrapped key へ変換する。
///
/// caller は対象 recipient の public key 境界を検証済みとして渡す責務を持つ。
/// 本関数は `ProtectedSecret` の借用中だけ key bytes を平文化し、平文 slice を返さない。
/// 返値は RSA-OAEP の ciphertext bytes であり、support 外では content key として解釈しない。
/// 暗号化失敗時は `Err` を返し、未 wrap の key material を fallback として露出しない。
pub(crate) fn rsa_oaep_encrypt(public: &RsaPublicKey, key: &ProtectedSecret) -> Result<Vec<u8>> {
    key.with_secret(|bytes| Ok(public.encrypt(&mut OsRng, Oaep::new::<Sha256>(), bytes)?))
}
