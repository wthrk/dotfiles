//! ゲスト内で起動するコマンドに、シナリオ共通の環境とログ形式を適用する。

use std::ffi::OsStr;
use std::process::{Command, Stdio};

use crate::{Result, runtime_env::ScenarioEnv};
use anyhow::bail;
use dotfiles_core::command as command_format;

/// シナリオ環境を反映したうえでコマンドを実行し、非 0 終了を失敗にする。
pub(crate) fn run_with_env<I, S>(env: Option<&ScenarioEnv>, program: &str, args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = command_format::os_strings(args);
    let mut command = Command::new(program);
    command.args(&args);
    if let Some(env) = env {
        env.apply_to(&mut command);
    }
    run_command(command, &command_format::display(program, &args))
}

/// 存在確認など失敗も観測対象になるコマンドを実行し、終了状態だけを返す。
pub(crate) fn status_with_env<I, S>(
    env: Option<&ScenarioEnv>,
    program: &str,
    args: I,
) -> Result<std::process::ExitStatus>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = command_format::os_strings(args);
    println!("$ {}", command_format::display(program, &args));
    let mut command = Command::new(program);
    command.args(&args);
    if let Some(env) = env {
        env.apply_to(&mut command);
    }
    Ok(command.status()?)
}

/// 進行を待つ間に繰り返す観測のため、終了状態だけを取り、出力はログへ流さない。
pub(crate) fn status_quiet(program: &str, args: &[&str]) -> Result<std::process::ExitStatus> {
    Ok(Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?)
}

/// ログインシェルへ渡す唯一のスクリプト本文。位置引数をそのまま `exec` する。
const EXEC_POSITIONAL_ARGS: &str = r#"exec -- "$@""#;

/// 対象ユーザーのログインシェルを経由して実行するための `sudo -H -u ...` 引数を作る。
///
/// `sudo -H -u` は HOME/USER/LOGNAME/SHELL を対象ユーザーのレコードから作り直すが、シェルを挟まない
/// 起動では起動ファイルが読まれず、PATH と Nix の profile 変数が実ログインと変わる。ログインシェルを
/// 挟むことで、これらは起動ファイル（zsh なら `/etc/zshenv`、sh なら `/etc/bashrc`）が読む
/// nix-darwin の `set-environment` から来るようになり、ハーネス側に写しを持たなくて済む。
///
/// `test_inputs` は `sudo` の env_reset で消える、テストが与えないと成立しない入力に限る。シェルの起動
/// ファイルより先に置くが、`set-environment` が上書きするのは PATH と Nix の profile 変数だけなので
/// 打ち消されない。
///
/// 実行対象は `-lc` のスクリプト本文へ埋め込まず、位置引数として渡す。シェルは
/// [`EXEC_POSITIONAL_ARGS`] しか解釈しないため、引数に空白や shell metacharacter が含まれても
/// 語分割・glob 展開・変数展開・コマンド置換のいずれも起きない。
pub(crate) fn sudo_login_shell_args(
    user: &str,
    login_shell: &str,
    test_inputs: &[(String, String)],
    program: &str,
    args: &[&str],
) -> Vec<String> {
    [
        "-H".to_string(),
        "-u".to_string(),
        user.to_string(),
        "/usr/bin/env".to_string(),
    ]
    .into_iter()
    .chain(
        test_inputs
            .iter()
            .map(|(key, value)| format!("{key}={value}")),
    )
    .chain([
        login_shell.to_string(),
        "-lc".to_string(),
        EXEC_POSITIONAL_ARGS.to_string(),
        login_shell.to_string(),
        program.to_string(),
    ])
    .chain(args.iter().map(|arg| (*arg).to_string()))
    .collect()
}

/// ログに出した表示と同じ `Command` を実行し、失敗時は終了状態をそのまま報告する。
fn run_command(mut command: Command, label: &str) -> Result<()> {
    println!("$ {label}");
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        bail!("command failed: {label}: {status}")
    }
}
