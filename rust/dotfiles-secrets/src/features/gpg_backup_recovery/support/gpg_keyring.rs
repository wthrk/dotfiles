//! gpgme key/subkey metadata の technical helper。

use anyhow::bail;

use crate::Result;

pub(crate) fn select_authentication_subkey<'key>(
    key: &'key gpgme::Key,
) -> Result<gpgme::Subkey<'key>> {
    let primary_fingerprint = required_gpgme_text(key.fingerprint(), "GPG key fingerprint")?;
    for subkey in key.subkeys() {
        if is_primary_subkey(&subkey, primary_fingerprint)? {
            continue;
        }
        if subkey.can_authenticate()
            && subkey.is_secret()
            && !subkey.is_revoked()
            && !subkey.is_expired()
            && !subkey.is_disabled()
        {
            return Ok(subkey);
        }
    }
    bail!("GPG authentication subkey could not be resolved")
}

pub(crate) fn is_primary_subkey(
    subkey: &gpgme::Subkey<'_>,
    primary_fingerprint: &str,
) -> Result<bool> {
    Ok(subkey
        .fingerprint()
        .map_err(|error| gpgme_text_error(error, "GPG subkey fingerprint"))?
        .eq_ignore_ascii_case(primary_fingerprint))
}

/// `gpgme` 0.11.0 `Key::fingerprint` / `Subkey::{fingerprint,keygrip}` は raw 値の欠落を
/// `Err(None)`、非 UTF-8 値を `Err(Some(Utf8Error))` で返す。
///
/// 出典: <https://docs.rs/crate/gpgme/0.11.0/source/src/keys.rs#L203-L206>,
/// <https://docs.rs/crate/gpgme/0.11.0/source/src/keys.rs#L318-L321>,
/// <https://docs.rs/crate/gpgme/0.11.0/source/src/keys.rs#L442-L445>。
/// どちらも「別の key/subkey」と推測せず opaque failure として伝播する。
pub(crate) fn required_gpgme_text<'a>(
    value: std::result::Result<&'a str, Option<std::str::Utf8Error>>,
    label: &'static str,
) -> Result<&'a str> {
    value.map_err(|error| gpgme_text_error(error, label))
}

fn gpgme_text_error(error: Option<std::str::Utf8Error>, label: &'static str) -> anyhow::Error {
    match error {
        Some(error) => anyhow::Error::new(error).context(format!("{label} is not valid UTF-8")),
        None => anyhow::anyhow!("{label} is absent"),
    }
}
