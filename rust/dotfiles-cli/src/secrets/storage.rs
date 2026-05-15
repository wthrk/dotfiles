//! YubiKey bootstrap secret storage の公開 API と責務分離した内部実装の結線。
//!
//! 呼び出し側の互換性を保つため、既存 `storage` モジュールの型と操作関数はここから
//! 再公開し、内部では model / wire / crypto / operations / tests を分離して保守する。

mod crypto;
mod model;
mod operations;
mod wire;

pub use model::{
    BootstrapSecrets, SecretBytes, SecretDevice, SecretName, YubikeyRole, secret_bytes,
};
pub(crate) use operations::secret_name;
pub use operations::{
    check_put_preconditions, check_rotate_preconditions, check_setup_preconditions, enroll, get,
    put, rotate_bws_token, setup, verify_local_storage,
};

#[cfg(test)]
mod tests;
