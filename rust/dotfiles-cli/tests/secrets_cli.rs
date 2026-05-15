//! `dotfiles secrets` の CLI 境界を実プロセスの TTY / pipe で検証する。
//!
//! YubiKey PIV 操作は `secrets-test-stub` feature の in-memory device に限定し、stdin、
//! stdout、stderr、TTY 判定、prompt 入力は実際のプロセス境界を通す。

use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};

type TestResult<T> = anyhow::Result<T>;

const STUB_FLAG: &str = "--test-stub-yubikey";
const TIMEOUT: Duration = Duration::from_secs(5);

struct CommandRun {
    success: bool,
    stdout: String,
    stderr: String,
}

struct PtyRun {
    success: bool,
    output: String,
}

#[test]
fn setup_runs_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe(["yubikey", "setup", "--serial", "2001"], None)?;

    assert!(run.success, "stderr: {}", run.stderr);
    Ok(())
}

#[test]
fn put_reads_non_tty_stdin_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe(
        [
            "yubikey",
            "put",
            "bws-access-token",
            "--serial",
            "2001",
            "--stdin",
        ],
        Some("new-token\r"),
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    Ok(())
}

#[test]
fn put_reads_tty_prompt_with_stub_yubikey() -> TestResult<()> {
    let run = run_pty(
        ["yubikey", "put", "bws-access-token", "--serial", "2001"],
        Some("new-token\n"),
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("bws-access-token: "));
    Ok(())
}

#[test]
fn get_writes_secret_to_pipe_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe(
        ["yubikey", "get", "bws-access-token", "--serial", "2001"],
        None,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert_eq!(run.stdout, "token");
    Ok(())
}

#[test]
fn get_refuses_secret_output_to_tty_with_stub_yubikey() -> TestResult<()> {
    let run = run_pty(
        ["yubikey", "get", "bws-access-token", "--serial", "2001"],
        None,
    )?;

    assert!(!run.success, "output: {}", run.output);
    assert!(
        run.output.contains("refusing to write secret to terminal"),
        "output: {}",
        run.output
    );
    Ok(())
}

#[test]
fn enroll_primary_reads_non_tty_stdin_json_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe(
        [
            "yubikey",
            "enroll-primary",
            "--serial",
            "2001",
            "--stdin-json",
        ],
        Some(bootstrap_json()),
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"role\": \"primary\""));
    assert!(run.stdout.contains("\"local_storage\": \"ok\""));
    Ok(())
}

#[test]
fn enroll_primary_reads_tty_prompts_with_stub_yubikey() -> TestResult<()> {
    let run = run_pty(
        ["yubikey", "enroll-primary", "--serial", "2001"],
        Some("u@example.com\rpw\rnew-token\r"),
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("bw-email: "));
    assert!(run.output.contains("bw-password: "));
    assert!(run.output.contains("bws-access-token: "));
    assert!(run.output.contains("\"role\": \"primary\""));
    Ok(())
}

#[test]
fn enroll_spare_reads_non_tty_stdin_json_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe(
        [
            "yubikey",
            "enroll-spare",
            "--primary-serial",
            "2001",
            "--spare-serial",
            "2002",
            "--stdin-json",
        ],
        Some(bootstrap_json()),
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"role\": \"spare\""));
    assert!(run.stdout.contains("\"serial\": 2002"));
    Ok(())
}

#[test]
fn enroll_spare_uses_stub_yubikey_without_secret_reentry() -> TestResult<()> {
    let run = run_pipe(
        [
            "yubikey",
            "enroll-spare",
            "--primary-serial",
            "2001",
            "--spare-serial",
            "2002",
        ],
        None,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"role\": \"spare\""));
    Ok(())
}

#[test]
fn rotate_bws_token_reads_non_tty_stdin_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe(
        ["yubikey", "rotate-bws-token", "--serial", "2001", "--stdin"],
        Some("new-token\r"),
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"serial\": 2001"));
    assert!(run.stdout.contains("\"local_storage\": \"ok\""));
    Ok(())
}

#[test]
fn rotate_bws_token_reads_tty_prompt_with_stub_yubikey() -> TestResult<()> {
    let run = run_pty(
        ["yubikey", "rotate-bws-token", "--serial", "2001"],
        Some("new-token\n"),
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("bws-access-token: "));
    assert!(run.output.contains("\"serial\": 2001"));
    Ok(())
}

#[test]
fn verify_yubikey_runs_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe(["verify-yubikey", "--serial", "2001"], None)?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"local_storage\": \"ok\""));
    assert!(run.stdout.contains("\"bws\": \"skipped\""));
    Ok(())
}

/// 非 TTY 実行では stdin/stdout/stderr を明示的に pipe/null へ接続し、TTY 判定を実際に変える。
fn run_pipe<const N: usize>(args: [&str; N], input: Option<&str>) -> TestResult<CommandRun> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dotfiles"));
    command
        .arg("secrets")
        .arg(STUB_FLAG)
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
        stdin.write_all(input.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    Ok(CommandRun {
        success: output.status.success(),
        stdout: String::from_utf8(output.stdout)?,
        stderr: String::from_utf8(output.stderr)?,
    })
}

/// PTY 実行では子プロセスを controlling TTY 付きで起動し、prompt と TTY stdout 拒否を検証する。
fn run_pty<const N: usize>(args: [&str; N], input: Option<&str>) -> TestResult<PtyRun> {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_dotfiles"));
    command.arg("secrets");
    command.arg(STUB_FLAG);
    command.args(args);
    let mut child = pair.slave.spawn_command(command)?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader()?;
    let output_handle = thread::spawn(move || {
        let mut output = String::new();
        reader.read_to_string(&mut output).map(|_| output)
    });

    if let Some(input) = input {
        let mut writer = pair.master.take_writer()?;
        writer.write_all(input.as_bytes())?;
        drop(writer);
    }

    let status = wait_pty_child(&mut child)?;
    drop(pair.master);
    let output = output_handle
        .join()
        .map_err(|_| anyhow::anyhow!("failed to join PTY output reader"))??;
    Ok(PtyRun {
        success: status.success(),
        output,
    })
}

/// prompt 待ちの失敗を検証の hang にしないため、PTY 子プロセスは期限付きで待つ。
fn wait_pty_child(
    child: &mut Box<dyn Child + Send + Sync>,
) -> TestResult<portable_pty::ExitStatus> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            child.kill()?;
            bail!("timed out waiting for PTY child process");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn bootstrap_json() -> &'static str {
    r#"{
  "bw-email": "u@example.com",
  "bw-password": "pw",
  "bws-access-token": "token"
}
"#
}
