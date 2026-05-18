//! `dotfiles secrets` application 層の DTO。

use super::super::support::protection::ProtectedSecret;

#[cfg(test)]
use super::super::domain::SecretName;

/// 登録に必要な 3 field を同じ保護 session で所有する。
pub(crate) struct EnrollmentSecretSet<'session> {
    pub(crate) bw_email: ProtectedSecret<'session>,
    pub(crate) bw_password: ProtectedSecret<'session>,
    pub(crate) bws_access_token: ProtectedSecret<'session>,
}

impl<'session> EnrollmentSecretSet<'session> {
    /// 同じ `SecretSession` に所属する 3 field から登録対象 secret を構築する。
    pub(crate) fn new(
        bw_email: ProtectedSecret<'session>,
        bw_password: ProtectedSecret<'session>,
        bws_access_token: ProtectedSecret<'session>,
    ) -> Self {
        Self {
            bw_email,
            bw_password,
            bws_access_token,
        }
    }

    #[cfg(test)]
    pub(crate) fn assert_secret_eq(&self, name: SecretName, expected: &[u8]) {
        match name {
            SecretName::BwEmail => self
                .bw_email
                .with_secret(|secret| assert_eq!(secret, expected)),
            SecretName::BwPassword => self
                .bw_password
                .with_secret(|secret| assert_eq!(secret, expected)),
            SecretName::BwsAccessToken => self
                .bws_access_token
                .with_secret(|secret| assert_eq!(secret, expected)),
        }
    }
}
