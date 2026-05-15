//! ゲスト内で起動するコマンドに、シナリオ共通の環境とログ形式を適用する。

use std::ffi::OsStr;
use std::process::Command;

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

/// 別ユーザーの HOME/USER/PATH を明示して実行するための `sudo -H -u ... env ...` 引数を作る。
pub(crate) fn sudo_user_args(
    user: &str,
    envs: &[(String, String)],
    program: &str,
    args: &[&str],
) -> Vec<String> {
    [
        "-H".to_string(),
        "-u".to_string(),
        user.to_string(),
        "env".to_string(),
    ]
    .into_iter()
    .chain(envs.iter().map(|(key, value)| format!("{key}={value}")))
    .chain(std::iter::once(program.to_string()))
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
