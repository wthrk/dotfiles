use std::ffi::OsStr;
use std::process::Command;

use crate::{Result, runtime_env::ScenarioEnv};
use anyhow::bail;
use dotfiles_core::command as command_format;

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

pub(crate) fn sudo_user_args(
    user: &str,
    envs: &[(String, String)],
    program: &str,
    args: &[&str],
) -> Vec<String> {
    let mut result = vec![
        "-H".to_string(),
        "-u".to_string(),
        user.to_string(),
        "env".to_string(),
    ];
    result.extend(envs.iter().map(|(key, value)| format!("{key}={value}")));
    result.push(program.to_string());
    result.extend(args.iter().map(|arg| (*arg).to_string()));
    result
}

fn run_command(mut command: Command, label: &str) -> Result<()> {
    println!("$ {label}");
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        bail!("command failed: {label}: {status}")
    }
}
