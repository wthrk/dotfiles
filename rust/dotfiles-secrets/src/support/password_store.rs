//! password-store filesystem backend の technical read operations。

use crate::{
    Result,
    domain::pass_restore::{PASSWORD_STORE_GPG_ID, PasswordStoreReadiness},
    support::filesystem,
};

fn store_root() -> Result<std::path::PathBuf> {
    filesystem::home_child(".password-store")
}

pub(crate) fn password_store_exists() -> Result<bool> {
    Ok(filesystem::path_exists_including_broken_symlink(
        &store_root()?,
    ))
}

pub(crate) fn inspect_password_store() -> Result<PasswordStoreReadiness> {
    let root = store_root()?;
    let gpg_id = root.join(PASSWORD_STORE_GPG_ID);
    let gpg_id_present = filesystem::is_regular_file(&gpg_id);
    let gpg_id_recipients = if gpg_id_present {
        filesystem::read_regular_text_lines(&gpg_id)?
    } else {
        Vec::new()
    };
    Ok(PasswordStoreReadiness {
        gpg_id_present,
        gpg_id_recipients,
        sample_entry: filesystem::first_regular_file_with_extension(&root, "gpg", ".git"),
    })
}
