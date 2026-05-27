//! `dotfiles secrets` の CLI 境界で、実機接続前に完結する validation を検証する。
//!
//! Production command path は実機 `real` route 固定で、テスト専用 stub/env 分岐を持たない。
//! そのため、この integration test は実機 YubiKey を要求しない停止条件だけを扱う。

use std::process::{Command, Stdio};

use anyhow::Context;

type TestResult<T> = anyhow::Result<T>;

const ADAPTER_ROUTE_AUDIT_PREFIX: &str = "DOTFILES_SECRETS_DEVICE_ADAPTER_ROUTE";
#[cfg(feature = "secrets-internal-test-stub")]
const INTERNAL_STUB_ENDPOINT_ENV: &str = "DOTFILES_SECRETS_INTERNAL_STUB_MOCKITO_URL";

struct CommandRun {
    success: bool,
    stdout: String,
    stderr: String,
}

/// `secrets-internal-test-stub` は mockito-backed adapter を compile-time injection して setup use case を検証する。
#[cfg(feature = "secrets-internal-test-stub")]
#[test]
fn setup_runs_with_mockito_internal_stub() -> TestResult<()> {
    let mut server = mockito::Server::new();
    let _open = server.mock("POST", "/devices/2001/open").create();
    let _key_exists = server
        .mock("GET", "/devices/2001/key-exists")
        .with_body(r#"{"value":false}"#)
        .create();
    let _version = server
        .mock("GET", "/devices/2001/piv-version")
        .with_body(r#"{"major":5,"minor":3,"patch":0}"#)
        .create();
    let _pin_retries = server
        .mock("GET", "/devices/2001/pin-retries")
        .with_body(r#"{"value":1}"#)
        .create();
    let _manifest_read = server
        .mock("GET", "/devices/2001/objects/6291222")
        .with_status(404)
        .expect(2)
        .create();
    let _bw_email_read = server
        .mock("GET", "/devices/2001/objects/6291223")
        .with_status(404)
        .create();
    let _bw_password_read = server
        .mock("GET", "/devices/2001/objects/6291224")
        .with_status(404)
        .create();
    let _bws_access_token_read = server
        .mock("GET", "/devices/2001/objects/6291225")
        .with_status(404)
        .create();
    let _auth = server
        .mock("POST", "/devices/2001/management-auth-preconditions")
        .create();
    let _generate = server.mock("POST", "/devices/2001/generate-key").create();
    let _write_manifest = server
        .mock("PUT", "/devices/2001/objects/6291222")
        .with_status(200)
        .create();

    let run = run_pipe_with_env(
        ["yubikey", "setup", "--serial", "2001"],
        None,
        &[(INTERNAL_STUB_ENDPOINT_ENV, server.url())],
    )?;

    assert!(
        run.success,
        "stdout: {}\nstderr: {}",
        run.stdout, run.stderr
    );
    assert!(
        run.stderr
            .contains(&format!("{ADAPTER_ROUTE_AUDIT_PREFIX}=stub")),
        "stderr: {}",
        run.stderr
    );
    Ok(())
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
#[cfg(not(feature = "secrets-internal-test-stub"))]
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
    run_pipe_with_env(args, input, &[])
}

fn run_pipe_with_env<const N: usize>(
    args: [&str; N],
    input: Option<&str>,
    envs: &[(&str, String)],
) -> TestResult<CommandRun> {
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
    for (key, value) in envs {
        command.env(key, value);
    }

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
