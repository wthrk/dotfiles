//! `dotfiles secrets` の実バイナリ（実機 YubiKey adapter）で実行する CLI 統合テスト。
//!
//! スタブ YubiKey を使う統合テストは `dotfiles-cli-secrets-test-stub` crate に分離する。
//! このファイルは `CARGO_BIN_EXE_dotfiles`（production binary）のみを参照し、
//! test double に依存しない。

use std::{
    io::{ErrorKind, Write},
    process::{Command, Stdio},
};

use anyhow::Context;

type TestResult<T> = anyhow::Result<T>;

struct CommandRun {
    success: bool,
    stdout: String,
    stderr: String,
}

fn run_pipe_real<I, S>(args: I, input: Option<&str>) -> TestResult<CommandRun>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut command = Command::new(env!("CARGO_BIN_EXE_dotfiles"));
    command
        .arg("secrets")
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn()?;
    if let Some(input) = input {
        let mut stdin = child.stdin.take().context("failed to open child stdin")?;
        match stdin.write_all(input.as_bytes()) {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::BrokenPipe => {}
            Err(err) => return Err(err.into()),
        }
    }

    let output = child.wait_with_output()?;
    Ok(CommandRun {
        success: output.status.success(),
        stdout: String::from_utf8(output.stdout)?,
        stderr: String::from_utf8(output.stderr)?,
    })
}

#[test]
fn put_rejects_non_tty_without_serial_with_real_yubikey_adapter() -> TestResult<()> {
    let run = run_pipe_real(
        ["yubikey", "put", "bws-access-token", "--stdin"],
        Some("secret\r"),
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("pass --serial in non-interactive use"));
    Ok(())
}

#[test]
fn verify_yubikey_real_adapter_flow_is_manual_and_can_be_skipped_or_blocked() -> TestResult<()> {
    let Some(serial) = std::env::var("DOTFILES_TEST_REAL_YUBIKEY_SERIAL").ok() else {
        eprintln!(
            "skipped: set DOTFILES_TEST_REAL_YUBIKEY_SERIAL to run real YubiKey adapter flow"
        );
        return Ok(());
    };
    if std::env::var("DOTFILES_RUN_REAL_YUBIKEY_TESTS").as_deref() != Ok("1") {
        eprintln!(
            "blocked: export DOTFILES_RUN_REAL_YUBIKEY_TESTS=1 for explicit real-device test execution"
        );
        return Ok(());
    }

    let run = run_pipe_real(["verify-yubikey", "--serial", serial.as_str()], None)?;
    assert!(
        run.success || run.stderr.contains("YubiKey PIN") || run.stderr.contains("failed"),
        "stdout: {}\nstderr: {}",
        run.stdout,
        run.stderr
    );
    Ok(())
}
