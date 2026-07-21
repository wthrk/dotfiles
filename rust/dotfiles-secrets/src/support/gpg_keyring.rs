//! gpgme key/subkey metadata の technical helper。

use anyhow::Context;

use crate::Result;

pub(crate) fn select_authentication_subkey<'key>(
    key: &'key gpgme::Key,
) -> Result<gpgme::Subkey<'key>> {
    let primary_fingerprint = key.fingerprint().ok().map(str::to_owned);
    key.subkeys()
        .filter(|subkey| !is_primary_subkey(subkey, primary_fingerprint.as_deref()))
        .find(|subkey| {
            subkey.can_authenticate()
                && subkey.is_secret()
                && !subkey.is_revoked()
                && !subkey.is_expired()
                && !subkey.is_disabled()
        })
        .context("GPG authentication subkey could not be resolved")
}

pub(crate) fn is_primary_subkey(
    subkey: &gpgme::Subkey<'_>,
    primary_fingerprint: Option<&str>,
) -> bool {
    matches!((subkey.fingerprint().ok(), primary_fingerprint), (Some(fingerprint), Some(primary)) if fingerprint.eq_ignore_ascii_case(primary))
}
