//! `dotfiles secrets` の storage 層。
//!
//! この層は secret storage のデータモデル、wire format、暗号処理、device trait
//! に対する保存操作を扱う。端末 I/O、process 保護、実機 YubiKey discovery には依存しない。

mod crypto;
mod model;
mod operations;
mod wire;

pub use model::{
    BootstrapSecretSource, BootstrapSecrets, PivObjectId, SecretBytes, SecretDevice, SecretName,
    VerifySummary, YubikeyRole,
};
pub use operations::{
    check_put_preconditions, check_rotate_preconditions, check_setup_preconditions, enroll, get,
    put, rotate_bws_token, setup, verify_local_storage,
};

#[cfg(test)]
mod tests;
