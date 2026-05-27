//! `dotfiles secrets` の CLI 境界で、実機接続前に完結する validation を検証する。
//!
//! Production command path は実機 `real` route 固定で、テスト専用 stub/env 分岐を持たない。
//! そのため、この integration test は実機 YubiKey を要求しない停止条件だけを扱う。

use std::process::{Command, Stdio};

use anyhow::Context;

type TestResult<T> = anyhow::Result<T>;

const ADAPTER_ROUTE_AUDIT_PREFIX: &str = "DOTFILES_SECRETS_DEVICE_ADAPTER_ROUTE";

struct CommandRun {
    success: bool,
    stdout: String,
    stderr: String,
}

/// `verify-yubikey` は非対話実行で serial 省略時に device I/O へ進まず失敗する。
#[test]
fn verify_yubikey_requires_serial_in_non_interactive_use() -> TestResult<()> {
    let run = run_pipe(["verify-yubikey"], None)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("pass --serial in non-interactive use"));
    Ok(())
}

/// `verify-yubikey` は production route を `real` として監査出力し、stub route を出力しない。
#[test]
fn verify_yubikey_audits_real_route_without_stub_selection() -> TestResult<()> {
    let run = run_pipe(["verify-yubikey"], None)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr
            .contains(&format!("{ADAPTER_ROUTE_AUDIT_PREFIX}=real")),
        "stderr: {}",
        run.stderr
    );
    assert!(
        !run.stderr
            .contains(&format!("{ADAPTER_ROUTE_AUDIT_PREFIX}=stub")),
        "stderr: {}",
        run.stderr
    );
    Ok(())
}

/// `verify-yubikey` は `--all` と `--check` の併用を device I/O 前に拒否する。
#[test]
fn verify_yubikey_rejects_all_with_check() -> TestResult<()> {
    let run = run_pipe(
        [
            "verify-yubikey",
            "--serial",
            "2001",
            "--all",
            "--check",
            "bws",
        ],
        None,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr
            .contains("--all and --check cannot be used together")
    );
    Ok(())
}

/// `put --stdin` は serial 必須条件を secret 入力や device I/O より先に評価する。
#[test]
fn put_stdin_requires_serial_before_reading_secret() -> TestResult<()> {
    let run = run_pipe(["yubikey", "put", "bws-access-token", "--stdin"], None)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("pass --serial in non-interactive use"));
    Ok(())
}

fn run_pipe<const N: usize>(args: [&str; N], input: Option<&str>) -> TestResult<CommandRun> {
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
        use std::io::Write as _;
        stdin.write_all(input.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    Ok(CommandRun {
        success: output.status.success(),
        stdout: String::from_utf8(output.stdout)?,
        stderr: String::from_utf8(output.stderr)?,
    })
}
