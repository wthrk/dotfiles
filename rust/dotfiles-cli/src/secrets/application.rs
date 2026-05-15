//! `dotfiles secrets` の application 層で保持する保護済み secret 状態。
//!
//! storage model は保存形式と secret 値そのものを表し、ここでは enroll / rotate などの
//! use-case 実行中だけ必要な memory lock 付き所有状態を扱う。

use super::{
    storage::{self, BootstrapSecretSource, BootstrapSecrets, SecretName},
    util::protection::{Protected, SecretMemoryGuard},
};
use crate::Result;

/// 単一 secret を memory lock guard と同じ生存期間で保持する use-case 状態。
pub(crate) type ProtectedSecret = Protected<storage::SecretBytes>;

/// bootstrap 登録が保存前に要求する 3 種類の保護済み secret。
pub(crate) struct ProtectedBootstrapSecrets {
    bw_email: ProtectedSecret,
    bw_password: ProtectedSecret,
    bws_access_token: ProtectedSecret,
}

impl ProtectedBootstrapSecrets {
    /// prompt 経路で field ごとに保護済みになった値だけを bootstrap 入力として受ける。
    pub(crate) fn new(
        bw_email: ProtectedSecret,
        bw_password: ProtectedSecret,
        bws_access_token: ProtectedSecret,
    ) -> Self {
        Self {
            bw_email,
            bw_password,
            bws_access_token,
        }
    }

    /// JSON や device 復号で得た bootstrap secret は field ごとに lock してから登録へ渡す。
    pub(crate) fn protect(
        secrets: BootstrapSecrets,
        memory: &SecretMemoryGuard,
    ) -> Result<ProtectedBootstrapSecrets> {
        Ok(ProtectedBootstrapSecrets {
            bw_email: protect_secret(secrets.bw_email, memory)?,
            bw_password: protect_secret(secrets.bw_password, memory)?,
            bws_access_token: protect_secret(secrets.bws_access_token, memory)?,
        })
    }
}

impl BootstrapSecretSource for ProtectedBootstrapSecrets {
    /// storage 登録中だけ、要求された bootstrap secret の平文 bytes を借用する。
    fn get(&self, name: SecretName) -> &[u8] {
        match name {
            SecretName::BwEmail => self.bw_email.as_slice(),
            SecretName::BwPassword => self.bw_password.as_slice(),
            SecretName::BwsAccessToken => self.bws_access_token.as_slice(),
        }
    }
}

/// 単一 secret は memory lock 付き状態にしてから storage 操作へ渡す。
pub(crate) fn protect_secret(
    secret: storage::SecretBytes,
    memory: &SecretMemoryGuard,
) -> Result<ProtectedSecret> {
    memory.protect_value(secret, storage::SecretBytes::as_slice)
}

/// command 入力境界で確定した単一 secret を memory lock 付き状態へ移す。
pub(crate) fn protect_secret_input(
    input: super::input::SecretInputBuffer,
    memory: &SecretMemoryGuard,
) -> Result<ProtectedSecret> {
    protect_secret(input.into(), memory)
}
