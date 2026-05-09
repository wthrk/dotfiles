//! ゲスト内シナリオで使う作業ディレクトリ、Nix 設定、ユーザー環境を正規化する。
//!
//! GitHub Actions では `GITHUB_WORKSPACE`、Tart では共有ボリューム、手元実行では current_dir を使う。
//! ユーザー切り替え時の HOME/PATH もここで作り、各 step が個別に推測しないようにする。

use std::env;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::Result;
use anyhow::{Context, bail};
use dotfiles_core::host;

/// シナリオ中の全コマンドが共有する作業場所と Nix 設定。
pub(crate) struct ScenarioEnv {
    pub(crate) workspace: PathBuf,
    pub(crate) runner_temp: PathBuf,
    pub(crate) nix_config: String,
}

impl ScenarioEnv {
    /// CI、Tart、手元実行の順で作業ディレクトリを決め、実行用一時ディレクトリを作る。
    pub(crate) fn current() -> Result<Self> {
        let guest_workspace = PathBuf::from("/Volumes/My Shared Files/repo");
        let workspace = env::var_os("GITHUB_WORKSPACE")
            .map(PathBuf::from)
            .or_else(|| {
                guest_workspace
                    .join("flake.nix")
                    .is_file()
                    .then_some(guest_workspace)
            })
            .unwrap_or(env::current_dir()?);
        let runner_temp = env::var_os("RUNNER_TEMP")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                if workspace.starts_with("/Volumes/My Shared Files/repo") {
                    env::var_os("HOME")
                        .map(PathBuf::from)
                        .unwrap_or_else(env::temp_dir)
                        .join("runner-temp")
                } else {
                    env::temp_dir().join("dotfiles-runner-temp")
                }
            });
        let nix_config = env::var("NIX_CONFIG")
            .unwrap_or_else(|_| "experimental-features = nix-command flakes".to_string());
        std::fs::create_dir_all(&runner_temp)?;
        Ok(Self {
            workspace,
            runner_temp,
            nix_config,
        })
    }

    /// 外部コマンドを検証対象 checkout 上で実行し、Nix 設定と一時ディレクトリを明示する。
    pub(crate) fn apply_to(&self, command: &mut Command) {
        command
            .current_dir(&self.workspace)
            .env("GITHUB_WORKSPACE", &self.workspace)
            .env("RUNNER_TEMP", &self.runner_temp)
            .env("NIX_CONFIG", &self.nix_config)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
    }
}

/// Tart 既定ユーザーでも GitHub Actions でも使えるよう、環境変数から現在ユーザー名を決める。
pub(crate) fn current_user() -> String {
    env::var("USER")
        .or_else(|_| env::var("LOGNAME"))
        .unwrap_or_else(|_| "admin".to_string())
}

/// `dotfiles init` と `dotfiles switch darwin` が同じ出力名を使うよう、短いホスト名を返す。
pub(crate) fn current_host() -> Result<String> {
    if let Ok(host) = env::var("HOST")
        && !host.is_empty()
    {
        return Ok(host::short(&host).to_string());
    }
    if let Ok(host) = env::var("HOSTNAME")
        && !host.is_empty()
    {
        return Ok(host::short(&host).to_string());
    }
    let output = Command::new("hostname").output()?;
    if !output.status.success() {
        bail!("hostname command failed");
    }
    let host = String::from_utf8(output.stdout)?;
    let host = host::short(host.trim());
    if host.is_empty() {
        bail!("host is required")
    } else {
        Ok(host.to_string())
    }
}

/// 現在ユーザーの `dotfiles init` が作る flake パスを返す。
pub(crate) fn local_config_flake_for_current_user() -> Result<PathBuf> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is required")?;
    Ok(local_config_flake_in(home))
}

/// `sudo -u` で作った別ユーザーのローカル flake パスを返す。
pub(crate) fn local_config_flake_for_user(user: &str) -> Result<PathBuf> {
    Ok(local_config_flake_in(user_home(user)?))
}

/// 指定ユーザーの設定ディレクトリを使い、ハードコードした `/Users/...` なしで flake ref を作る。
pub(crate) fn local_config_ref(user: &str, output: &str) -> Result<String> {
    Ok(format!(
        "{}#{output}",
        local_config_dir_for_user(user)?.display()
    ))
}

/// macOS directory services から解決したホーム配下の dotfiles 設定ディレクトリを返す。
pub(crate) fn local_config_dir_for_user(user: &str) -> Result<PathBuf> {
    Ok(user_home(user)?.join(".config/dotfiles"))
}

/// 現在ユーザーは `$HOME`、別ユーザーは `dscl` の `NFSHomeDirectory` からホームを解決する。
pub(crate) fn user_home(user: &str) -> Result<PathBuf> {
    if user == current_user()
        && let Some(home) = env::var_os("HOME")
    {
        return Ok(PathBuf::from(home));
    }

    dscl_home(user)?.with_context(|| format!("NFSHomeDirectory is required for user {user}"))
}

/// 解決済みホームから `~/.config/dotfiles/flake.nix` のパスを組み立てる。
fn local_config_flake_in(home: PathBuf) -> PathBuf {
    home.join(".config/dotfiles/flake.nix")
}

/// 2 人目の Home Manager 検証で使う、ログインに近い最小環境を作る。
pub(crate) fn dotfilesci_env(nix_config: &str) -> Result<Vec<(String, String)>> {
    user_env("dotfilesci", "/bin/zsh", nix_config, None)
}

/// Darwin switch 後のユーザー環境に近い PATH を渡し、対象ユーザーとして評価と確認を行う。
pub(crate) fn ya_env(nix_config: &str) -> Result<Vec<(String, String)>> {
    user_env(
        "ya",
        "/etc/profiles/per-user/ya/bin/zsh",
        nix_config,
        Some(
            "/etc/profiles/per-user/ya/bin:/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin:/usr/bin:/bin:/usr/sbin:/sbin",
        ),
    )
}

/// `sudo -u` で環境が引き継がれすぎないよう、HOME/USER/LOGNAME/SHELL/NIX_CONFIG/PATH を明示する。
fn user_env(
    user: &str,
    shell: &str,
    nix_config: &str,
    path: Option<&str>,
) -> Result<Vec<(String, String)>> {
    let mut env = vec![
        ("HOME".to_string(), user_home(user)?.display().to_string()),
        ("USER".to_string(), user.to_string()),
        ("LOGNAME".to_string(), user.to_string()),
        ("SHELL".to_string(), shell.to_string()),
        ("NIX_CONFIG".to_string(), nix_config.to_string()),
    ];
    if let Some(path) = path {
        env.push(("PATH".to_string(), path.to_string()));
    }
    Ok(env)
}

/// `/Users/<name>` を仮定せず、macOS が持つユーザーレコードからホームを読む。
fn dscl_home(user: &str) -> Result<Option<PathBuf>> {
    let output = Command::new("dscl")
        .args([".", "-read", &format!("/Users/{user}"), "NFSHomeDirectory"])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8(output.stdout)?;
    Ok(stdout.split_whitespace().nth(1).map(PathBuf::from))
}
