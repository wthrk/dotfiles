//! gpgme concrete keyring backend operations.

use crate::{
    Result,
    domain::{
        gpg_backup::PrimaryFingerprint,
        gpg_restore::{
            ImportedKeyComposition, Keygrip, OpenSshPublicKey, ResolvedSubkey, SubkeyCapability,
        },
        pass_restore::GpgRecipientId,
    },
    support::{
        gpg_keyring::{is_primary_subkey, required_gpgme_text, select_authentication_subkey},
        protection::{ProtectedSecret, gpg_backup as backup_protection},
    },
};
use anyhow::Context;
use gpgme::{ExportMode, Protocol};
fn context() -> Result<gpgme::Context> {
    gpgme::Context::from_protocol(Protocol::OpenPgp).context("failed to create gpgme context")
}
pub(crate) fn export_secret_key(primary: &PrimaryFingerprint) -> Result<ProtectedSecret> {
    backup_protection::export_secret_key(primary.as_str())
}
pub(crate) fn parse_backup_primary_fingerprint(
    backup: &ProtectedSecret,
) -> Result<PrimaryFingerprint> {
    PrimaryFingerprint::parse(&backup_protection::parse_primary_fingerprint_hex(backup)?)
}
pub(crate) fn secret_key_exists(primary: &PrimaryFingerprint) -> Result<bool> {
    let mut context = context()?;
    match context.get_secret_key(primary.as_str()) {
        Ok(_) => Ok(true),
        Err(error) if error.code() == gpgme::Error::NO_SECKEY.code() => Ok(false),
        Err(error) => Err(anyhow::Error::new(error).context("failed to query GPG secret key")),
    }
}
pub(crate) fn import_secret_key(backup: &ProtectedSecret) -> Result<PrimaryFingerprint> {
    PrimaryFingerprint::parse(&backup_protection::import_secret_key(backup)?)
}
pub(crate) fn delete_secret_key(primary: &PrimaryFingerprint) -> Result<()> {
    let mut context = context()?;
    let key = context
        .get_secret_key(primary.as_str())
        .context("failed to resolve GPG secret key for rollback deletion")?;
    context
        .delete_secret_key(&key)
        .context("failed to delete GPG secret key during rollback")
}
pub(crate) fn inspect_imported_key(primary: &PrimaryFingerprint) -> Result<ImportedKeyComposition> {
    let mut context = context()?;
    let key = context
        .get_secret_key(primary.as_str())
        .context("failed to resolve imported GPG key")?;
    let primary_fp = required_gpgme_text(key.fingerprint(), "GPG key fingerprint")?;
    let mut subkeys = Vec::new();
    for subkey in key.subkeys() {
        if is_primary_subkey(&subkey, primary_fp)? {
            continue;
        }
        let usable = subkey.is_secret()
            && !subkey.is_revoked()
            && !subkey.is_expired()
            && !subkey.is_disabled();
        if subkey.can_encrypt() {
            subkeys.push(ResolvedSubkey {
                capability: SubkeyCapability::Encryption,
                usable,
            });
        }
        if subkey.can_authenticate() {
            subkeys.push(ResolvedSubkey {
                capability: SubkeyCapability::Authentication,
                usable,
            });
        }
        if subkey.can_sign() {
            subkeys.push(ResolvedSubkey {
                capability: SubkeyCapability::Signing,
                usable,
            });
        }
    }
    Ok(ImportedKeyComposition::new(key.has_secret(), subkeys))
}
pub(crate) fn authentication_subkey_keygrip(primary: &PrimaryFingerprint) -> Result<Keygrip> {
    let mut context = context()?;
    let key = context
        .get_secret_key(primary.as_str())
        .context("failed to resolve imported GPG key")?;
    let selected = select_authentication_subkey(&key)?;
    Keygrip::parse(required_gpgme_text(
        selected.keygrip(),
        "GPG authentication subkey keygrip",
    )?)
}
pub(crate) fn authentication_subkey_ssh_public_key(
    primary: &PrimaryFingerprint,
) -> Result<OpenSshPublicKey> {
    let mut context = context()?;
    let subkey_fingerprint = {
        let key = context
            .get_secret_key(primary.as_str())
            .context("failed to resolve imported GPG key")?;
        let selected = select_authentication_subkey(&key)?;
        required_gpgme_text(
            selected.fingerprint(),
            "GPG authentication subkey fingerprint",
        )?
        .to_owned()
    };
    let mut data = gpgme::Data::new().context("failed to allocate gpgme ssh export buffer")?;
    context
        .export(
            [format!("{subkey_fingerprint}!")],
            ExportMode::SSH,
            &mut data,
        )
        .context("failed to export GPG authentication subkey as OpenSSH public key")?;
    let text = String::from_utf8(
        data.try_into_bytes()
            .context("failed to read exported OpenSSH public key bytes")?,
    )
    .context("exported OpenSSH public key is not valid UTF-8")?;
    OpenSshPublicKey::parse(
        text.lines()
            .find(|line| !line.trim().is_empty())
            .context("exported OpenSSH public key is empty")?,
    )
}
pub(crate) fn secret_key_available_for_recipient(recipient: &GpgRecipientId) -> Result<bool> {
    let mut context = context()?;
    match context.get_secret_key(recipient.as_str()) {
        Ok(_) => Ok(true),
        Err(error) if error.code() == gpgme::Error::NO_SECKEY.code() => Ok(false),
        Err(error) => Err(anyhow::Error::new(error)
            .context("failed to query GPG secret key for password-store recipient")),
    }
}
pub(crate) fn can_decrypt_store_entry(entry_path: &std::path::Path) -> Result<()> {
    backup_protection::verify_can_decrypt(entry_path)
}
