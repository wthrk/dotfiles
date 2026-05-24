//! `dotfiles secrets` の実行境界で、実機不要な入力検証経路を統合テストする。
//!
//! production 側へ test double を戻さず、YubiKey 系 use case の主要ガード条件を
//! CLI 経由で自動検証する。

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::Context;

type TestResult<T> = anyhow::Result<T>;

struct CommandRun {
    success: bool,
    stdout: String,
    stderr: String,
}

#[test]
fn put_rejects_non_tty_without_serial() -> TestResult<()> {
    let run = run_pipe(
        ["yubikey", "put", "bws-access-token", "--stdin"],
        Some("secret\n"),
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("pass --serial in non-interactive use"));
    Ok(())
}

#[test]
fn get_rejects_non_tty_without_serial() -> TestResult<()> {
    let run = run_pipe(["yubikey", "get", "bws-access-token"], Some(""))?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("pass --serial in non-interactive use"));
    Ok(())
}

#[test]
fn setup_rejects_non_tty_without_serial() -> TestResult<()> {
    let run = run_pipe(["yubikey", "setup"], Some(""))?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("pass --serial in non-interactive use"));
    Ok(())
}

#[test]
fn enroll_primary_rejects_non_tty_without_serial() -> TestResult<()> {
    let run = run_pipe(["yubikey", "enroll-primary"], Some(""))?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("pass --serial in non-interactive use"));
    Ok(())
}

#[test]
fn enroll_spare_rejects_non_tty_without_primary_serial() -> TestResult<()> {
    let run = run_pipe(["yubikey", "enroll-spare"], Some(""))?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr
            .contains("pass --primary-serial in non-interactive use")
    );
    Ok(())
}

#[test]
fn enroll_spare_rejects_non_tty_without_spare_serial_in_stdin_json_mode() -> TestResult<()> {
    let run = run_pipe(
        [
            "yubikey",
            "enroll-spare",
            "--stdin-json",
            "--primary-serial",
            "123456",
        ],
        Some("{\"bw_email\":\"a\",\"bw_password\":\"b\",\"bws_access_token\":\"c\"}\n"),
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("pass --spare-serial in non-interactive use"));
    Ok(())
}

#[test]
fn rotate_rejects_non_tty_without_serial() -> TestResult<()> {
    let run = run_pipe(["yubikey", "rotate-bws-token"], Some(""))?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("pass --stdin in non-interactive use"));
    Ok(())
}

#[test]
fn verify_rejects_non_tty_without_serial() -> TestResult<()> {
    let run = run_pipe(["verify-yubikey"], Some(""))?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("pass --serial in non-interactive use"));
    Ok(())
}

#[test]
fn rotate_with_stdin_rejects_non_tty_without_serial() -> TestResult<()> {
    let run = run_pipe(
        ["yubikey", "rotate-bws-token", "--stdin"],
        Some("rotated-token\n"),
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("pass --serial in non-interactive use"));
    Ok(())
}

fn run_pipe<I, S>(args: I, stdin_text: Option<&str>) -> TestResult<CommandRun>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut command = Command::new(dotfiles_bin());
    command.args(["secrets"]);
    for arg in args {
        command.arg(arg.as_ref());
    }
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .context("failed to spawn dotfiles command")?;

    if let Some(input) = stdin_text {
        let mut stdin = child.stdin.take().context("missing child stdin")?;
        stdin
            .write_all(input.as_bytes())
            .context("failed to write stdin")?;
    }

    let output = child
        .wait_with_output()
        .context("failed to wait for child output")?;

    Ok(CommandRun {
        success: output.status.success(),
        stdout: String::from_utf8(output.stdout).context("stdout is not UTF-8")?,
        stderr: String::from_utf8(output.stderr).context("stderr is not UTF-8")?,
    })
}

fn dotfiles_bin() -> &'static str {
    env!("CARGO_BIN_EXE_dotfiles")
}
