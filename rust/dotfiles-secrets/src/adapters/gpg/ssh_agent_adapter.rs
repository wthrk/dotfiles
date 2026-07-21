//! `SshAgentPort` を gpg-agent の SSH key list（`sshcontrol`）と SSH agent socket 観測へ接続する adapter。
//!
//! authentication subkey の keygrip を `${GNUPGHOME:-$HOME/.gnupg}/sshcontrol` へ冪等に登録し、SSH support
//! 利用可否を「SSH agent socket（`S.gpg-agent.ssh`）が解決でき、その socket 経路で agent が列挙する identity に
//! 復元鍵が含まれる」状態として観測して domain 値（`SshAgentReadiness`）へ翻訳する。`gpgconf` CLI は使わず、
//! socket は `${GNUPGHOME:-$HOME/.gnupg}/S.gpg-agent.ssh` を優先候補として確認し、その path が socket でない場合
//! だけ既存環境変数 `SSH_AUTH_SOCK` が指す socket へ fallback する（`config/zsh/env.zsh` の上書き条件と同じ前提）。
//! identity の列挙は、解決した socket へ接続して SSH agent protocol（`SSH_AGENTC_REQUEST_IDENTITIES`）で公開鍵
//! identity を列挙し、各 identity の key blob を期待公開鍵（`OpenSshPublicKey`）の key blob と byte 一致で照合して、
//! 復元鍵 identity が識別可能かを判定する。設計 L83 に従い、復元鍵と無関係な既存 identity の有無は観測しない。
//! identity comment（gpg-agent は `cardno:` / `openpgp:` 等を載せ keygrip とは限らない）は鍵同一性に使えないため
//! 照合に用いない。SSH support 充足の業務判定そのものは domain（`SshAgentReadiness::ensure_ready` /
//! `OpenSshPublicKey::matches_agent_key_blob`）へ残す。

use std::{fs::OpenOptions, io::Write};

use anyhow::Context;

use crate::{
    Result,
    domain::gpg_restore::{Keygrip, OpenSshPublicKey, SshAgentReadiness},
    ports::gpg::SshAgentPort,
    support::{
        adapter_backend::SshAgentBackend,
        ssh_agent_protocol::{request_identities, sshcontrol_contains, sshcontrol_path},
        ssh_agent_socket::{gnupg_home, resolve_ssh_agent_socket},
    },
};

impl SshAgentPort for SshAgentBackend {
    fn register_authentication_subkey(&mut self, keygrip: &Keygrip) -> Result<()> {
        let path = sshcontrol_path(gnupg_home()?);
        if sshcontrol_contains(&path, keygrip)? {
            // 既登録ならその状態を維持する（冪等）。
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .context("failed to create GnuPG home directory for sshcontrol")?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .context("failed to open gpg-agent sshcontrol")?;
        writeln!(file, "{}", keygrip.as_str())
            .context("failed to register keygrip in gpg-agent sshcontrol")?;
        Ok(())
    }

    fn inspect_ssh_agent(
        &mut self,
        expected_public_key: &OpenSshPublicKey,
    ) -> Result<SshAgentReadiness> {
        // 固定 path が socket ならそれ、socket でなければ既存 `SSH_AUTH_SOCK` が socket ならそれを使う。
        // `gnupg_home()` 解決失敗（例: `HOME` 未設定）は socket 無しへ握り潰さず、その実原因を `?` で伝播する。
        let socket = resolve_ssh_agent_socket()?;
        let socket_resolved = socket.is_some();
        // 解決した socket へ接続して SSH agent protocol で公開鍵 identity を列挙し、各 identity の key blob を
        // 期待公開鍵の key blob と byte 一致で照合する。一致する identity があれば復元鍵が識別可能であり、
        // socket が解決できない、または接続/列挙に失敗した場合は false とし、復元鍵を識別できないことで
        // 停止させる（識別不能を「識別可能」へ倒さない）。設計 L83 に従い、復元鍵と無関係な既存 identity の
        // 有無は観測しない。
        let recovery_identity_present = match socket {
            Some(path) => request_identities(&path)
                .map(|blobs| {
                    blobs
                        .iter()
                        .any(|blob| expected_public_key.matches_agent_key_blob(blob))
                })
                .unwrap_or(false),
            None => false,
        };
        Ok(SshAgentReadiness {
            socket_resolved,
            recovery_identity_present,
        })
    }
}
