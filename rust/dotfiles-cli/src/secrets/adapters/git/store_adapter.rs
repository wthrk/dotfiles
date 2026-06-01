//! `PasswordStorePort` を `~/.password-store` の filesystem 観測へ接続する adapter。
//!
//! `$HOME` を解決して `~/.password-store` の存在確認と、clone 後 store root の識別ファイル
//! （`.gpg-id`）の有無を観測し、domain 値（`PasswordStoreReadiness`）へ翻訳する。store 既存時の停止可否や
//! store 可読性の充足判定そのものの業務規則は domain（`PasswordStoreReadiness::ensure_readable`）に残す。
//! ここでは filesystem 走査だけを担い、`pass` CLI への無条件シェルアウトはしない。

use crate::{
    Result,
    secrets::{
        adapters::git::password_store_path,
        domain::pass_restore::{PASSWORD_STORE_GPG_ID, PasswordStoreReadiness},
    },
};

/// `~/.password-store` の filesystem 観測を `PasswordStorePort` 契約へ翻訳する adapter。
#[derive(Default)]
pub(super) struct PasswordStoreAdapter;

impl PasswordStoreAdapter {
    /// `~/.password-store` が既に存在するか（path として存在するか）を確認する。
    pub(super) fn password_store_exists(&self) -> Result<bool> {
        Ok(password_store_path()?.exists())
    }

    /// clone 先 store root を走査し、`.gpg-id` の有無を `PasswordStoreReadiness` へ翻訳する。
    pub(super) fn inspect_password_store(&self) -> Result<PasswordStoreReadiness> {
        let gpg_id = password_store_path()?.join(PASSWORD_STORE_GPG_ID);
        Ok(PasswordStoreReadiness {
            gpg_id_present: gpg_id.is_file(),
        })
    }
}
