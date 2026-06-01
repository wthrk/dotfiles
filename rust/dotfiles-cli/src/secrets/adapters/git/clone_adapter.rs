//! `GitClonePort` を git2 + libssh2 の SSH agent 認証 clone へ接続する adapter。
//!
//! private `password-store` repository を `~/.password-store` へ clone する。認証は git2 の credentials
//! callback で libssh2 の SSH agent 経路（`Cred::ssh_key_from_agent`）を使い、gpg-agent の SSH support
//! が提示する GPG authentication subkey 由来の identity だけを利用する。`git` CLI と GitHub API は使わない。
//! clone は提示する SSH identity を選べないため、socket 解決は gpg-agent socket
//! （`${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh`）を strict に使う `resolve_gpg_agent_socket` を用い、通常の
//! `ssh-agent` を指しうる既存 `SSH_AUTH_SOCK` へは fallback しない。既存 `~/.ssh/id_ed25519` を新規運用で使わ
//! ない仕様（spec L92 / L100 / L210）を守るためであり、gpg-agent socket が無ければ clone を停止する。
//!
//! `Cred::ssh_key_from_agent` は username だけを受け取り、agent 内の特定 identity を選んで提示する API を
//! 持たない。gpg-agent の SSH socket は `sshcontrol` に登録された keygrip（= GPG authentication subkey 由来
//! identity）だけを露出するため、strict gpg-agent socket を使えば通常の `ssh-agent` 鍵は提示されない。ただし
//! `sshcontrol` に複数 identity が登録されていれば agent 側で別 identity を提示しうるため、単一鍵限定はこの
//! adapter だけでは担保できない。そこで application 側が clone 前に「この gpg-agent socket が復元した GPG
//! authentication subkey の identity を提示している」ことを #14 の key blob 照合で確定し、満たさなければ clone
//! へ進ませない。本 adapter は strict gpg-agent socket への固定と clone 翻訳だけを担い、identity 照合は
//! application + `SshAgentPort` 側で担保する。clone URL の妥当性判断は domain（`PasswordStoreRemote`）に委ねる。

use anyhow::Context;
use git2::{Cred, CredentialType, FetchOptions, RemoteCallbacks, build::RepoBuilder};

use crate::{
    Result,
    secrets::{
        adapters::git::password_store_path, domain::pass_restore::PasswordStoreRemote,
        support::ssh_agent_socket::resolve_gpg_agent_socket,
    },
};

/// git2 の SSH agent 認証 clone を `GitClonePort` 契約へ翻訳する adapter。
#[derive(Default)]
pub(super) struct GitCloneAdapter;

impl GitCloneAdapter {
    /// 検証済み clone URL を `~/.password-store` へ SSH agent 認証で clone する。
    pub(super) fn clone_password_store(&mut self, remote: &PasswordStoreRemote) -> Result<()> {
        // clone は提示する SSH identity を選べないため、gpg-agent socket を strict に解決する。通常の `ssh-agent`
        // を指しうる `SSH_AUTH_SOCK` へは fallback せず、gpg-agent socket が無ければ clone を試みず停止する
        // （既存 `~/.ssh` 鍵での clone を防ぐ。spec L92 / L100 / L210）。
        let socket = resolve_gpg_agent_socket()
            .context("could not resolve the gpg-agent SSH agent socket for password-store clone")?;
        // libssh2 は credentials callback で SSH agent を使う前に `SSH_AUTH_SOCK` を参照する。strict に解決した
        // gpg-agent socket へ環境変数を合わせ、`git2` が gpg-agent SSH 経路を使うようにする。clone は process-global
        // な `SSH_AUTH_SOCK` を一時的に上書きするだけであり、後続の同一 `dotfiles` process 操作へ副作用を残さない
        // よう、旧値を保存して clone の成功/失敗いずれでも scope 離脱時に必ず復元する。
        let previous_sock = std::env::var_os("SSH_AUTH_SOCK");
        // SAFETY: clone 実行は単一スレッドの use case 経路であり、set/restore はいずれも非 secret な socket path
        // （`SSH_AUTH_SOCK`）だけを扱う。本 process の SSH agent 接続先を strict 解決した gpg-agent socket へ一時
        // 固定し、scope 離脱で旧値（未設定なら除去）へ戻すためだけに行う。
        unsafe {
            std::env::set_var("SSH_AUTH_SOCK", &socket);
        }
        let _restore_sock = scopeguard::guard(previous_sock, |previous| {
            // SAFETY: 上書きと同じ単一スレッド経路での復元であり、扱う値は非 secret な socket path だけ。
            unsafe {
                match previous {
                    Some(value) => std::env::set_var("SSH_AUTH_SOCK", value),
                    None => std::env::remove_var("SSH_AUTH_SOCK"),
                }
            }
        });

        let mut callbacks = RemoteCallbacks::new();
        callbacks.credentials(|_url, username_from_url, allowed_types| {
            // SSH の username は GitHub では `git` 固定。URL から取れない場合も `git` を使う。
            let username = username_from_url.unwrap_or("git");
            if allowed_types.contains(CredentialType::SSH_KEY) {
                // GPG authentication subkey 由来 identity は gpg-agent の SSH agent から提示される。
                Cred::ssh_key_from_agent(username)
            } else {
                Err(git2::Error::from_str(
                    "password-store clone requires SSH agent authentication",
                ))
            }
        });

        let mut fetch_options = FetchOptions::new();
        fetch_options.remote_callbacks(callbacks);

        let destination = password_store_path()?;
        RepoBuilder::new()
            .fetch_options(fetch_options)
            .clone(remote.as_str(), &destination)
            .map_err(|error| {
                anyhow::anyhow!("failed to clone private password-store over SSH: {error}")
            })?;
        Ok(())
    }
}
