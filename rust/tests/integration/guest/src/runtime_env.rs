//! ゲスト内シナリオで使う作業ディレクトリ、Nix 設定、ユーザーレコードの参照を正規化する。
//!
//! GitHub Actions では `GITHUB_WORKSPACE`、Tart では共有ボリューム、手元実行では current_dir を使う。
//! ユーザーのホームとログインシェルは macOS のユーザーレコードから読み、各 step が個別に推測しない
//! ようにする。

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::Result;
use anyhow::{Context, bail};
use dotfiles_core::{host, path::display as path_str};

/// シナリオ中の全コマンドが共有する作業場所と Nix 設定。
pub(crate) struct ScenarioEnv {
    pub(crate) workspace: PathBuf,
    pub(crate) bootstrap_script: PathBuf,
    pub(crate) dotfiles_source: String,
    pub(crate) pass_source_to_bootstrap: bool,
    bootstrap_source_ref_env: Option<String>,
    runner_temp: PathBuf,
    nix_config: String,
}

impl ScenarioEnv {
    /// CI、Tart、手元実行の順で作業ディレクトリを決め、実行用一時ディレクトリを作る。
    pub(crate) fn current(source_hash: Option<String>) -> Result<Self> {
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
        let source = RuntimeSource::new(&workspace, &runner_temp, source_hash.as_deref())?;
        Ok(Self {
            workspace,
            bootstrap_script: source.bootstrap_script,
            dotfiles_source: source.dotfiles_source,
            pass_source_to_bootstrap: source.pass_source_to_bootstrap,
            bootstrap_source_ref_env: source.bootstrap_source_ref_env,
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
        if let Some(source_ref) = &self.bootstrap_source_ref_env {
            command.env("DOTFILES_BOOTSTRAP_SOURCE_REF", source_ref);
        }
    }

    /// `sudo` の env_reset を越えて対象ユーザーへ渡す、テスト側が決める入力を返す。
    ///
    /// 対象ユーザーの HOME/USER/LOGNAME/SHELL は `sudo -H -u` がユーザーレコードから作り直し、
    /// PATH と Nix の profile 変数はログインシェルの起動ファイルが読む `set-environment` が作る。
    /// ここに並べるのはそうした環境の写しではなく、CI から与えないと成立しない入力だけである。
    pub(crate) fn test_inputs(&self) -> Vec<(String, String)> {
        [("NIX_CONFIG".to_string(), self.nix_config.clone())]
            .into_iter()
            .chain(self.bootstrap_source_ref_env.as_ref().map(|source_ref| {
                (
                    "DOTFILES_BOOTSTRAP_SOURCE_REF".to_string(),
                    source_ref.clone(),
                )
            }))
            .collect()
    }
}

struct RuntimeSource {
    bootstrap_script: PathBuf,
    dotfiles_source: String,
    pass_source_to_bootstrap: bool,
    bootstrap_source_ref_env: Option<String>,
}

impl RuntimeSource {
    fn new(workspace: &Path, runner_temp: &Path, source_hash: Option<&str>) -> Result<Self> {
        let Some(source_hash) = source_hash else {
            return Ok(Self {
                bootstrap_script: workspace.join("scripts/bootstrap.sh"),
                dotfiles_source: path_str(workspace),
                pass_source_to_bootstrap: true,
                bootstrap_source_ref_env: None,
            });
        };
        if source_hash.trim().is_empty() {
            bail!("source hash must not be empty");
        }

        let dotfiles_source = format!("github:wthrk/dotfiles/{source_hash}");
        let bootstrap_script = runner_temp.join("bootstrap.sh");
        let url = format!(
            "https://raw.githubusercontent.com/wthrk/dotfiles/{source_hash}/scripts/bootstrap.sh"
        );
        let status = Command::new("curl")
            .args(["-fsSL", "-o"])
            .arg(&bootstrap_script)
            .arg(&url)
            .status()?;
        if !status.success() {
            bail!("failed to download bootstrap.sh: {url}: {status}");
        }
        fs::set_permissions(&bootstrap_script, fs::Permissions::from_mode(0o755))?;

        Ok(Self {
            bootstrap_script,
            dotfiles_source: dotfiles_source.clone(),
            pass_source_to_bootstrap: false,
            bootstrap_source_ref_env: Some(source_hash.to_string()),
        })
    }
}

/// Tart 既定ユーザーでも GitHub Actions でも使えるよう、環境変数から現在ユーザー名を決める。
pub(crate) fn current_user() -> String {
    env::var("USER")
        .or_else(|_| env::var("LOGNAME"))
        .unwrap_or_else(|_| "admin".to_string())
}

/// `dotfiles init` が `--host` 省略時に書く出力名と同じ短いホスト名を返す。
///
/// CLI は `hostname` だけを見る。シナリオは `--host` を渡さずに生成した flake の出力名を参照するので、
/// ここで環境変数を優先すると CLI が書いた名前と食い違い、存在しない属性を検査することになる。
pub(crate) fn current_host() -> Result<String> {
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

/// 対象ユーザーのログインシェルを、`login(1)` と `sudo -i` が使うのと同じユーザーレコードから読む。
///
/// シナリオはこのシェルをログインシェルとして起動し、PATH と Nix の profile 変数を、起動ファイルが
/// 読む nix-darwin の `set-environment` から受け取る。固定値にすると、実ユーザーが読む起動ファイルと
/// 検証が読む起動ファイルが食い違う。
pub(crate) fn login_shell(user: &str) -> Result<String> {
    dscl_read(user, "UserShell")?.with_context(|| format!("UserShell is required for user {user}"))
}

/// `/Users/<name>` を仮定せず、macOS が持つユーザーレコードからホームを読む。
fn dscl_home(user: &str) -> Result<Option<PathBuf>> {
    Ok(dscl_read(user, "NFSHomeDirectory")?.map(PathBuf::from))
}

/// macOS のユーザーレコードから 1 属性を読む。レコードが無ければ `None`。
fn dscl_read(user: &str, key: &str) -> Result<Option<String>> {
    let output = Command::new("dscl")
        .args([".", "-read", &format!("/Users/{user}"), key])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8(output.stdout)?;
    Ok(stdout.split_whitespace().nth(1).map(str::to_string))
}
