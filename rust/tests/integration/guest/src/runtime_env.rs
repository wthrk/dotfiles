//! ゲスト内シナリオで使う作業ディレクトリ、Nix 設定、ユーザーレコードの参照を正規化する。
//!
//! 作業ディレクトリは起動時の checkout（current_dir）で、実行環境では切り替えない。
//! ユーザーのホームとログインシェルは macOS のユーザーレコードから読み、各 step が個別に推測しない
//! ようにする。

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::Result;
use anyhow::{Context, bail};
use dotfiles_core::host;

/// bootstrap script を置く作業ディレクトリ。
///
/// シナリオは `sudo -u` で作った別ユーザーからもこの script を起動するため、実行ユーザーのホーム内
/// ではなく全ユーザーが辿れる場所に置く。
const RUNNER_TEMP: &str = "/tmp/dotfiles-runtime-integration";

/// シナリオ中の全コマンドが共有する作業場所と Nix 設定。
pub(crate) struct ScenarioEnv {
    pub(crate) workspace: PathBuf,
    pub(crate) bootstrap_script: PathBuf,
    pub(crate) dotfiles_source: String,
    bootstrap_source_ref: String,
    nix_config: String,
}

impl ScenarioEnv {
    /// 検証対象 commit から bootstrap script と flake 参照を用意し、以後の手順で共有する。
    pub(crate) fn current(source_hash: &str) -> Result<Self> {
        let source_hash = source_hash.trim();
        if source_hash.is_empty() {
            bail!("source hash must not be empty");
        }

        let runner_temp = PathBuf::from(RUNNER_TEMP);
        fs::create_dir_all(&runner_temp)?;
        // bootstrap script は対象ユーザーへ降格してから起動するので、経路上の directory も読める
        // 必要がある。
        fs::set_permissions(&runner_temp, fs::Permissions::from_mode(0o755))?;

        Ok(Self {
            workspace: env::current_dir()?,
            bootstrap_script: download_bootstrap_script(&runner_temp, source_hash)?,
            dotfiles_source: format!("github:wthrk/dotfiles/{source_hash}"),
            bootstrap_source_ref: source_hash.to_string(),
            nix_config: env::var("NIX_CONFIG")
                .unwrap_or_else(|_| "experimental-features = nix-command flakes".to_string()),
        })
    }

    /// 外部コマンドを検証対象 checkout 上で実行し、Nix 設定と検証対象 commit を明示する。
    pub(crate) fn apply_to(&self, command: &mut Command) {
        command
            .current_dir(&self.workspace)
            .env("NIX_CONFIG", &self.nix_config)
            .env("DOTFILES_BOOTSTRAP_SOURCE_REF", &self.bootstrap_source_ref)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
    }

    /// `sudo` の env_reset を越えて対象ユーザーへ渡す、テスト側が決める入力を返す。
    ///
    /// 対象ユーザーの HOME/USER/LOGNAME/SHELL は `sudo -H -u` がユーザーレコードから作り直し、
    /// PATH と Nix の profile 変数はログインシェルの起動ファイルが読む `set-environment` が作る。
    /// ここに並べるのはそうした環境の写しではなく、テストが与えないと成立しない入力だけである。
    pub(crate) fn test_inputs(&self) -> Vec<(String, String)> {
        vec![
            ("NIX_CONFIG".to_string(), self.nix_config.clone()),
            (
                "DOTFILES_BOOTSTRAP_SOURCE_REF".to_string(),
                self.bootstrap_source_ref.clone(),
            ),
        ]
    }
}

/// 検証対象 commit の bootstrap script を取得する。利用者が `curl` で入手するのと同じ経路。
fn download_bootstrap_script(runner_temp: &Path, source_hash: &str) -> Result<PathBuf> {
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
    Ok(bootstrap_script)
}

/// 現在ユーザー名をユーザーレコードと同じ出所（ログインシェルが設定する環境）から読む。
pub(crate) fn current_user() -> Result<String> {
    env::var("USER")
        .or_else(|_| env::var("LOGNAME"))
        .context("USER or LOGNAME is required")
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
    if user == current_user()?
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
