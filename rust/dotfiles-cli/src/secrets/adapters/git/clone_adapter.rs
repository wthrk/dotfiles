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
//! authentication subkey の identity を提示している」ことを #14 の key blob 照合で確定し、さらに `sshcontrol`
//! が復元鍵の keygrip だけを持つ（別 authentication subkey が登録されていない）ことを `SshAgentPort` 経由で
//! 確定してから clone へ進ませる。本 adapter は strict gpg-agent socket への固定と clone 翻訳だけを担い、
//! identity 照合と single-key 担保は application + `SshAgentPort` 側で行う。clone URL の妥当性判断は domain
//! （`PasswordStoreRemote`）に委ねる。
//!
//! host key 検証: 新規マシンでは `~/.ssh/known_hosts` に `github.com` の host key が無いのが通常であり、
//! credentials だけでは MITM を防げない。`RemoteCallbacks::certificate_check` で、接続先 hostname が
//! `github.com` であり、libssh2 が提示する SSH host key の raw bytes が GitHub 公表の既知 host key
//! （Ed25519 / ECDSA / RSA の公開鍵）と byte 一致することを検証し、一致しなければ clone を停止する。
//! GitHub 公表鍵は出典コメント付きの定数 [`GITHUB_SSH_HOST_KEYS`] として pin する。

use anyhow::Context;
use git2::{
    CertificateCheckStatus, Cred, CredentialType, FetchOptions, RemoteCallbacks,
    build::RepoBuilder, cert::Cert,
};

use crate::{
    Result,
    secrets::{
        adapters::git::password_store_path, domain::pass_restore::PasswordStoreRemote,
        support::ssh_agent_socket::resolve_gpg_agent_socket,
    },
};

/// 接続を許可する GitHub の hostname。`PasswordStoreRemote` は `git@github.com:` 固定形式だけを許可する
/// ため、clone 先 host は常に `github.com` であり、それ以外の host へ提示された証明書は検証対象外として停止する。
const GITHUB_HOST: &str = "github.com";

/// GitHub が公表する SSH host key（OpenSSH 公開鍵本体の base64）と key type 名の pin。
///
/// 出典: GitHub `https://api.github.com/meta` の `ssh_keys`（および GitHub docs
/// "GitHub's SSH key fingerprints"）。2026-06-01 時点の公表値。各要素の 2 番目フィールド（base64 本体）は
/// libssh2 が `CertHostkey::hostkey()` で返す raw host key bytes を base64 化したものと一致する。新規マシンで
/// `known_hosts` に依存せず host を pin するためにこの定数で照合し、一致しなければ clone を停止する（MITM 防止）。
/// `hostkey_type()` の OpenSSH 名（`name()`）と本 base64 の type prefix を併せて照合し、type と鍵の両方一致を要求する。
const GITHUB_SSH_HOST_KEYS: [&str; 3] = [
    // ssh-ed25519
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl",
    // ecdsa-sha2-nistp256
    "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBEmKSENjQEezOmxkZMy7opKgwFB9nkt5YRrYMjNuG5N87uRgg6CLrbo5wAdT/y6v0mKV0U2w0WZ2YB/++Tpockg=",
    // ssh-rsa
    "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQCj7ndNxQowgcQnjshcLrqPEiiphnt+VTTvDP6mHBL9j1aNUkY4Ue1gvwnGLVlOhGeYrnZaMgRK6+PKCUXaDbC7qtbW8gIkhL7aGCsOr/C56SJMy/BCZfxd1nWzAOxSDPgVsmerOBYfNqltV9/hWCqBywINIR+5dIg6JTJ72pcEpEjcYgXkE2YEFXV1JHnsKgbLWNlhScqb2UmyRkQyytRLtL+38TGxkxCflmO+5Z8CSSNY7GidjMIZ7Q4zMjA2n1nGrlTDkzwDCsw+wqFPGQA179cnfGWOWRVruj16z6XyvxvjJwbz0wQZ75XK5tKSb7FNyeIEs4TT4jk+S4dhPeAUC5y+bDYirYgM4GC7uEnztnZyaVWQ7B381AK4Qdrwt51ZqExKbQpTUNn+EjqoTwvqNj4kqx5QUCI0ThS/YkOxJCXmPUWZbhjpCg56i+2aB6CmK2JGhn57K5mj0MNdBXA4/WnwH6XoPWJzK5Nyu2zB3nAZp+S5hpQs+p1vN1/wsjk=",
];

/// git2 の SSH agent 認証 clone を `GitClonePort` 契約へ翻訳する adapter。
#[derive(Default)]
pub(super) struct GitCloneAdapter;

impl GitCloneAdapter {
    /// 検証済み clone URL を `~/.password-store` へ SSH agent 認証で clone する。
    ///
    /// clone は `~/.password-store` の sibling（同一 parent dir = 同一 filesystem）に作る一意な temp
    /// directory 経由で原子的に行う。成功時のみ `~/.password-store` がまだ不在であることを再確認して
    /// `std::fs::rename` で temp を昇格させ、失敗時は temp を best-effort で削除して destination を残さない。
    /// 別 process が手順 3 の不存在確認後に `~/.password-store` を作っていた場合（TOCTOU）でも、その既存
    /// store は決して上書き・削除せず、temp を削除して error を返す。これにより本 adapter は「Err なら
    /// destination に何も残さない／Ok なら destination は今 clone した store だけ」を保証し、application 側の
    /// clone 失敗時 rollback（既存 store を誤削除しうる）を不要にする。
    pub(super) fn clone_password_store(&mut self, remote: &PasswordStoreRemote) -> Result<()> {
        // clone は提示する SSH identity を選べないため、gpg-agent socket を strict に解決する。通常の `ssh-agent`
        // を指しうる `SSH_AUTH_SOCK` へは fallback せず、gpg-agent socket が無ければ clone を試みず停止する
        // （既存 `~/.ssh` 鍵での clone を防ぐ。spec L92 / L100 / L210）。
        let socket = resolve_gpg_agent_socket()?
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
        // host key 検証: 新規マシンでは `known_hosts` に github.com が無いのが通常のため、GitHub 公表 host key
        // との byte 一致で host を pin する。hostname が github.com でない、host key を取得できない、または
        // pin 鍵と一致しない場合は MITM の可能性として clone を停止する（`CertificatePassthrough` で
        // known_hosts へ委譲しない）。
        callbacks.certificate_check(|cert, hostname| {
            match verify_github_host_key(cert, hostname) {
                Ok(()) => Ok(CertificateCheckStatus::CertificateOk),
                Err(message) => Err(git2::Error::from_str(&message)),
            }
        });

        let mut fetch_options = FetchOptions::new();
        fetch_options.remote_callbacks(callbacks);

        let destination = password_store_path()?;
        // destination の sibling（同一 parent = 同一 filesystem）に一意な temp directory を構える。system
        // temp dir は別 filesystem になりうり `rename` が原子的でなくなるため使わない。process id を含めた
        // 名前で他 process との衝突を避け、過去の異常終了で残った同名 temp があれば先に掃除する。
        let parent = destination.parent().ok_or_else(|| {
            anyhow::anyhow!("could not resolve the parent directory of ~/.password-store")
        })?;
        let temp_dir = parent.join(format!(".password-store.clone.{}.tmp", std::process::id()));
        if let Err(error) = remove_dir_all_if_present(&temp_dir) {
            return Err(error
                .context("failed to clean up a stale temporary password-store clone directory"));
        }

        // temp directory へ clone する。失敗時は temp を best-effort で削除し、destination には何も残さない。
        if let Err(error) = RepoBuilder::new()
            .fetch_options(fetch_options)
            .clone(remote.as_str(), &temp_dir)
        {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err(anyhow::anyhow!(
                "failed to clone private password-store over SSH: {error}"
            ));
        }

        // clone 成功後、destination がまだ不在であることを再確認してから temp を rename で昇格させる。手順 3 の
        // 不存在確認後に別 process が `~/.password-store` を作っていた場合（TOCTOU）は、その既存 store を決して
        // 上書きせず temp を削除して停止する。
        if destination.symlink_metadata().is_ok() {
            let _ = std::fs::remove_dir_all(&temp_dir);
            anyhow::bail!(
                "~/.password-store appeared during clone; refusing to overwrite a store created by another process"
            );
        }
        if let Err(error) = std::fs::rename(&temp_dir, &destination) {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err(anyhow::Error::new(error)
                .context("failed to move the cloned password-store into ~/.password-store"));
        }
        Ok(())
    }
}

/// path（directory）が存在すれば削除し、不在なら成功扱いにする。temp clone directory の事前掃除に使う。
fn remove_dir_all_if_present(path: &std::path::Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(anyhow::Error::new(error)),
    }
}

/// 接続先 host が `github.com` であり、提示された SSH host key が GitHub 公表の pin 鍵と一致するかを検証する。
///
/// hostname が `github.com` でない、提示された証明書が SSH host key でない、libssh2 が raw host key を返さない、
/// または key type / raw bytes が pin（[`GITHUB_SSH_HOST_KEYS`]）のいずれとも一致しない場合は、MITM の可能性
/// として `Err(message)` を返し clone を停止させる。一致した場合だけ `Ok(())` を返す。`known_hosts` へは委譲しない。
fn verify_github_host_key(cert: &Cert<'_>, hostname: &str) -> std::result::Result<(), String> {
    if hostname != GITHUB_HOST {
        return Err(format!(
            "refusing to clone: unexpected SSH host '{hostname}', only {GITHUB_HOST} is allowed"
        ));
    }
    let hostkey = cert
        .as_hostkey()
        .ok_or_else(|| "refusing to clone: server did not present an SSH host key".to_owned())?;
    let raw = hostkey
        .hostkey()
        .ok_or_else(|| "refusing to clone: SSH host key is unavailable for pinning".to_owned())?;
    let type_name = hostkey
        .hostkey_type()
        .ok_or_else(|| "refusing to clone: SSH host key type is unknown".to_owned())?
        .name();
    // key type 名と raw bytes の両方一致を要求する。type prefix が一致しても raw bytes が pin と異なる host は拒否する。
    let matches_pinned = GITHUB_SSH_HOST_KEYS.iter().any(|pinned| {
        let mut fields = pinned.split_whitespace();
        let pinned_type = fields.next();
        let pinned_body = fields.next();
        match (pinned_type, pinned_body) {
            (Some(pinned_type), Some(pinned_body)) => {
                pinned_type == type_name
                    && standard_base64_decode(pinned_body).is_some_and(|decoded| decoded == raw)
            }
            _ => false,
        }
    });
    if matches_pinned {
        Ok(())
    } else {
        Err(format!(
            "refusing to clone: {GITHUB_HOST} SSH host key did not match GitHub's published host keys (possible MITM)"
        ))
    }
}

/// standard base64 文字列を bytes へ decode する。pin した GitHub host key body の照合専用。
///
/// この adapter は base64 crate へ依存しないため、pin 値の decode に必要な最小 decoder を持つ。入力長は 4 の
/// 倍数でなければならず（canonical 長）、末尾以外の chunk に `=` を含めることはできない。末尾 chunk が満たない
/// 場合は `=` による canonical padding（`==` で 1 byte / `=` で 2 byte）を要求し、padding 位置・桁数が不正な
/// 値は拒否する。長さが 4 の倍数で末尾が完全な 4 文字 chunk なら padding は不要であり、その場合 `=` は現れない。
/// alphabet / padding / 長さのいずれの妥当性違反も `None` を返し、照合側で「一致しない」へ倒す。
fn standard_base64_decode(input: &str) -> Option<Vec<u8>> {
    let bytes = input.as_bytes();
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut output = Vec::with_capacity(bytes.len() / 4 * 3);
    let chunk_count = bytes.len() / 4;
    for (chunk_index, chunk) in bytes.chunks(4).enumerate() {
        let is_last_chunk = chunk_index + 1 == chunk_count;
        let mut accumulator = 0u32;
        let mut chunk_padding = 0usize;
        for (index, &symbol) in chunk.iter().enumerate() {
            if symbol == b'=' {
                // padding は末尾 chunk の末尾位置にだけ許可する。
                if !is_last_chunk || index < 2 {
                    return None;
                }
                chunk_padding += 1;
                accumulator <<= 6;
                continue;
            }
            if chunk_padding != 0 {
                return None;
            }
            let sextet = standard_base64_symbol_value(symbol)?;
            accumulator = (accumulator << 6) | u32::from(sextet);
        }
        let bytes_in_chunk = 3 - chunk_padding;
        let chunk_bytes = accumulator.to_be_bytes();
        output.extend_from_slice(&chunk_bytes[1..=bytes_in_chunk]);
    }
    Some(output)
}

/// standard base64 alphabet の 1 文字を 6-bit 値へ変換する。
fn standard_base64_symbol_value(symbol: u8) -> Option<u8> {
    match symbol {
        b'A'..=b'Z' => Some(symbol - b'A'),
        b'a'..=b'z' => Some(symbol - b'a' + 26),
        b'0'..=b'9' => Some(symbol - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    //! host key pin 照合（base64 decode と type/bytes 一致判定）という adapter 翻訳ロジックの単体テスト。
    //!
    //! libssh2 host key 提示を伴わない純粋な decode と pin 突合だけを検証し、外部 network/git は呼ばない。
    //! `Cert` は実 git 接続なしに構築できないため、pin 突合に使う decode 関数と pin 定数の整合だけを直接検証する。

    use super::{GITHUB_SSH_HOST_KEYS, standard_base64_decode};

    /// pin した各 GitHub host key の base64 本体が decode でき、type prefix を持つことを確認する。
    #[test]
    fn pinned_github_host_keys_decode() {
        for pinned in GITHUB_SSH_HOST_KEYS {
            let mut fields = pinned.split_whitespace();
            let key_type = fields.next().expect("pinned key has a type prefix");
            let body = fields.next().expect("pinned key has a base64 body");
            assert!(key_type.starts_with("ssh-") || key_type.starts_with("ecdsa-"));
            let decoded = standard_base64_decode(body).expect("pinned key body decodes");
            assert!(!decoded.is_empty());
        }
    }

    /// base64 decode の padding / alphabet 妥当性検証（不正値は `None`）を確認する。
    #[test]
    fn standard_base64_decode_rejects_invalid_input() {
        // 長さが 4 の倍数でない（canonical padding を欠いた truncated 入力）。
        assert!(standard_base64_decode("AAA").is_none());
        // alphabet 外の文字。
        assert!(standard_base64_decode("AA*A").is_none());
        // 不正 padding 位置。
        assert!(standard_base64_decode("A=AA").is_none());
        // 妥当な padding 付き値は decode できる（"Zm9v" == "foo"）。
        assert_eq!(standard_base64_decode("Zm9v"), Some(b"foo".to_vec()));
        assert_eq!(standard_base64_decode("Zg=="), Some(b"f".to_vec()));
    }

    /// canonical padding を持つ GitHub 形式の host key body は decode でき、その body から `=` を 1 文字
    /// 削った（4 の倍数でない truncated / 不正 padding）入力は契約どおり拒否することを確認する。
    #[test]
    fn standard_base64_decode_accepts_padded_github_key_and_rejects_truncated() {
        // ssh-ed25519 host key body は標準 base64 で末尾に `=` padding を持たない（長さが 4 の倍数）。
        let ed25519_body = GITHUB_SSH_HOST_KEYS[0]
            .split_whitespace()
            .nth(1)
            .expect("ed25519 pin has a base64 body");
        assert!(standard_base64_decode(ed25519_body).is_some());
        // ecdsa host key body は末尾に `=` padding を持つ canonical 標準 base64。
        let ecdsa_body = GITHUB_SSH_HOST_KEYS[1]
            .split_whitespace()
            .nth(1)
            .expect("ecdsa pin has a base64 body");
        assert!(ecdsa_body.ends_with('='), "ecdsa pin body is padded");
        assert!(standard_base64_decode(ecdsa_body).is_some());
        // padding を 1 文字削ると長さが 4 の倍数でなくなり、契約どおり拒否される。
        let truncated = &ecdsa_body[..ecdsa_body.len() - 1];
        assert!(standard_base64_decode(truncated).is_none());
    }
}
