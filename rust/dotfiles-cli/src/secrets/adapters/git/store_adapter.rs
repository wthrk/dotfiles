//! `PasswordStorePort` を `~/.password-store` の filesystem 観測へ接続する adapter。
//!
//! `$HOME` を解決して `~/.password-store` の存在確認と、clone 後 store root の識別ファイル
//! （`.gpg-id`）の有無・recipient 行・復号確認用サンプル `*.gpg` entry を観測し、domain 値
//! （`PasswordStoreReadiness`）へ翻訳する。検証失敗時のロールバック削除も filesystem 操作として担う。
//! recipient 形式妥当性・復号可否・store 既存時の停止可否といった業務規則は domain
//! （`PasswordStoreReadiness::parse_recipients` ほか）と keyring 照合へ残す。ここでは filesystem 走査と
//! best-effort 削除だけを担い、`.gpg-id` の中身解釈や `pass` CLI への無条件シェルアウトはしない。

use std::path::{Path, PathBuf};

use anyhow::Context;

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

    /// clone 先 store root を走査し、`.gpg-id` の有無・recipient 行・サンプル entry を
    /// `PasswordStoreReadiness` へ翻訳する。
    ///
    /// `.gpg-id` の各行は空行・`#` コメントを除いて未 trim のまま recipient 候補として渡し、形式妥当性の
    /// 判定は domain へ委ねる。サンプル entry は store 内の最初に見つかった `*.gpg` を 1 件だけ返し、復号
    /// 確認（keyring 照合）の対象にする。entry が 1 件も無い store でも `.gpg-id` 妥当性までは検証できる。
    pub(super) fn inspect_password_store(&self) -> Result<PasswordStoreReadiness> {
        let store_root = password_store_path()?;
        let gpg_id_path = store_root.join(PASSWORD_STORE_GPG_ID);
        let gpg_id_present = gpg_id_path.is_file();
        let gpg_id_recipients = if gpg_id_present {
            read_gpg_id_recipients(&gpg_id_path)?
        } else {
            Vec::new()
        };
        let sample_entry = find_sample_entry(&store_root);
        Ok(PasswordStoreReadiness {
            gpg_id_present,
            gpg_id_recipients,
            sample_entry,
        })
    }

    /// clone で作成した `~/.password-store` を best-effort で削除する（不在なら成功扱い）。
    pub(super) fn remove_password_store(&mut self) -> Result<()> {
        let store_root = password_store_path()?;
        match std::fs::remove_dir_all(&store_root) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(anyhow::Error::new(error)
                .context("failed to remove ~/.password-store during restore-pass rollback")),
        }
    }
}

/// `.gpg-id` の各行を読み、空行と `#` コメントを除いた行（未 trim）を recipient 候補として返す。
fn read_gpg_id_recipients(gpg_id_path: &Path) -> Result<Vec<String>> {
    let contents =
        std::fs::read_to_string(gpg_id_path).context("failed to read password-store .gpg-id")?;
    Ok(contents
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .map(str::to_owned)
        .collect())
}

/// store tree から最初に見つかった `*.gpg` entry の path を 1 件だけ返す（無ければ `None`）。
///
/// `.git` などの隠しディレクトリは pass entry を含まないため走査から除外する。復号確認に使う
/// サンプルを 1 件得るための浅い走査であり、全 entry の列挙はしない。
fn find_sample_entry(store_root: &Path) -> Option<PathBuf> {
    let mut stack = vec![store_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                let is_hidden = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with('.'));
                if !is_hidden {
                    stack.push(path);
                }
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("gpg") {
                return Some(path);
            }
        }
    }
    None
}
