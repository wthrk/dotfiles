#![cfg(feature = "secrets-internal-test-stub")]
//! `dotfiles secrets` の CLI 境界を feature-gated internal backend stub で検証する。
//!
//! Production command path は runtime env による real/stub 選択を持たない。この test target は
//! port ごとの初期条件 spec JSON を env で渡し、CLI 実行後に port ごとの最終状態観測 JSON だけを
//! 検証する。

use std::{
    io::{ErrorKind, Read, Write},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::Context;
use dotfiles_cli::secrets_internal_test_stub_contract::{
    BWS_STUB_SPEC_ENV, STUB_OBSERVATION_PREFIX, YUBIKEY_STUB_SPEC_ENV,
};
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};
use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(15);
const PRIMARY_SERIAL: u32 = 2001;
const SPARE_SERIAL: u32 = 2002;

type TestResult<T> = anyhow::Result<T>;

struct CommandRun {
    success: bool,
    stdout: String,
    stderr: String,
}

impl CommandRun {
    fn user_stdout(&self) -> String {
        strip_observation_lines(&self.stdout)
    }

    fn final_yubikey(&self) -> TestResult<Value> {
        final_observation(&self.stdout, "yubikey")
    }

    fn final_bws(&self) -> TestResult<Value> {
        final_observation(&self.stdout, "bws")
    }

    fn has_bws_observation(&self) -> bool {
        has_observation(&self.stdout, "bws")
    }
}

struct PtyRun {
    success: bool,
    output: String,
}

impl PtyRun {
    fn final_yubikey(&self) -> TestResult<Value> {
        final_observation(&self.output, "yubikey")
    }
}

struct StubPorts {
    yubikey_spec: Value,
    bws_spec_value: Value,
}

impl StubPorts {
    fn new(yubikey_spec: Value, bws_spec_value: Value) -> Self {
        Self {
            yubikey_spec,
            bws_spec_value,
        }
    }

    fn apply_to_command(&self, command: &mut Command) -> TestResult<()> {
        command
            .env(
                YUBIKEY_STUB_SPEC_ENV,
                serde_json::to_string(&self.yubikey_spec)?,
            )
            .env(
                BWS_STUB_SPEC_ENV,
                serde_json::to_string(&self.bws_spec_value)?,
            );
        Ok(())
    }

    fn apply_to_pty_command(&self, command: &mut CommandBuilder) -> TestResult<()> {
        command.env(
            YUBIKEY_STUB_SPEC_ENV,
            serde_json::to_string(&self.yubikey_spec)?,
        );
        command.env(
            BWS_STUB_SPEC_ENV,
            serde_json::to_string(&self.bws_spec_value)?,
        );
        Ok(())
    }
}

#[test]
fn setup_runs_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([fresh_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["yubikey", "setup", "--serial", "2001"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    Ok(())
}

#[test]
fn put_reads_non_tty_stdin_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([writable_bws_access_token_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(
        [
            "yubikey",
            "put",
            "bws-access-token",
            "--serial",
            "2001",
            "--stdin",
        ],
        Some("new-token\r"),
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert_stored_secret(
        &run.final_yubikey()?,
        PRIMARY_SERIAL,
        "bws-access-token",
        "new-token\r",
    );
    Ok(())
}

#[test]
fn put_reads_tty_prompt_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([writable_bws_access_token_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pty_with_stub(
        ["yubikey", "put", "bws-access-token", "--serial", "2001"],
        Some("new-token\n"),
        &stub,
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("bws-access-token: "));
    assert_stored_secret(
        &run.final_yubikey()?,
        PRIMARY_SERIAL,
        "bws-access-token",
        "new-token",
    );
    Ok(())
}

#[test]
fn get_writes_secret_to_pipe_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(
        ["yubikey", "get", "bws-access-token", "--serial", "2001"],
        None,
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert_eq!(run.user_stdout(), "token");
    Ok(())
}

#[test]
fn get_refuses_secret_output_to_tty_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pty_with_stub(
        ["yubikey", "get", "bws-access-token", "--serial", "2001"],
        None,
        &stub,
    )?;

    assert!(!run.success, "output: {}", run.output);
    assert!(run.output.contains("refusing to write secret to terminal"));
    Ok(())
}

#[test]
fn enroll_primary_reads_non_tty_stdin_json_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([fresh_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(
        [
            "yubikey",
            "enroll-primary",
            "--serial",
            "2001",
            "--stdin-json",
        ],
        Some(bootstrap_json()),
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"role\": \"primary\""));
    assert!(stdout.contains("\"name\": \"local-storage\""));
    assert!(stdout.contains("\"status\": \"ok\""));
    let final_yubikey = run.final_yubikey()?;
    assert_stored_secret(&final_yubikey, PRIMARY_SERIAL, "bw-email", "u@example.com");
    assert_stored_secret(&final_yubikey, PRIMARY_SERIAL, "bw-password", "pw");
    assert_stored_secret(&final_yubikey, PRIMARY_SERIAL, "bws-access-token", "token");
    Ok(())
}

#[test]
fn enroll_primary_reads_tty_prompts_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([fresh_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pty_with_stub(
        ["yubikey", "enroll-primary", "--serial", "2001"],
        Some("u@example.com\npw\ntoken\n"),
        &stub,
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("bw-email: "));
    assert!(run.output.contains("bw-password: "));
    assert!(run.output.contains("bws-access-token: "));
    assert!(run.output.contains("\"role\": \"primary\""));
    let final_yubikey = run.final_yubikey()?;
    assert_stored_secret(&final_yubikey, PRIMARY_SERIAL, "bw-email", "u@example.com");
    assert_stored_secret(&final_yubikey, PRIMARY_SERIAL, "bw-password", "pw");
    assert_stored_secret(&final_yubikey, PRIMARY_SERIAL, "bws-access-token", "token");
    Ok(())
}

#[test]
fn enroll_spare_reads_non_tty_stdin_json_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([
            fresh_device_spec(PRIMARY_SERIAL),
            fresh_device_spec(SPARE_SERIAL),
        ]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(
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
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"role\": \"spare\""));
    assert!(stdout.contains("\"serial\": 2002"));
    let final_yubikey = run.final_yubikey()?;
    assert_stored_secret(&final_yubikey, SPARE_SERIAL, "bw-email", "u@example.com");
    assert_stored_secret(&final_yubikey, SPARE_SERIAL, "bw-password", "pw");
    assert_stored_secret(&final_yubikey, SPARE_SERIAL, "bws-access-token", "token");
    Ok(())
}

#[test]
fn enroll_spare_without_secret_reentry() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([
            provisioned_device_spec(PRIMARY_SERIAL),
            fresh_device_spec(SPARE_SERIAL),
        ]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(
        [
            "yubikey",
            "enroll-spare",
            "--primary-serial",
            "2001",
            "--spare-serial",
            "2002",
        ],
        None,
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.user_stdout().contains("\"role\": \"spare\""));
    let final_yubikey = run.final_yubikey()?;
    assert_stored_secret(&final_yubikey, SPARE_SERIAL, "bw-email", "u@example.com");
    assert_stored_secret(&final_yubikey, SPARE_SERIAL, "bw-password", "pw");
    assert_stored_secret(&final_yubikey, SPARE_SERIAL, "bws-access-token", "token");
    Ok(())
}

#[test]
fn rotate_bws_token_reads_non_tty_stdin_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(
        ["yubikey", "rotate-bws-token", "--serial", "2001", "--stdin"],
        Some("new-token\r"),
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"serial\": 2001"));
    assert!(stdout.contains("\"name\": \"local-storage\""));
    assert!(stdout.contains("\"status\": \"ok\""));
    assert_stored_secret(
        &run.final_yubikey()?,
        PRIMARY_SERIAL,
        "bws-access-token",
        "new-token\r",
    );
    Ok(())
}

#[test]
fn rotate_bws_token_reads_tty_prompt_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pty_with_stub(
        ["yubikey", "rotate-bws-token", "--serial", "2001"],
        Some("new-token\n"),
        &stub,
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("bws-access-token: "));
    assert!(run.output.contains("\"serial\": 2001"));
    assert_stored_secret(
        &run.final_yubikey()?,
        PRIMARY_SERIAL,
        "bws-access-token",
        "new-token",
    );
    Ok(())
}

#[test]
fn rotate_bws_token_can_continue_to_another_tty_selected_yubikey() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([
            provisioned_device_spec(PRIMARY_SERIAL),
            provisioned_device_spec(SPARE_SERIAL),
        ]),
        bws_spec(),
    );
    let run = run_pty_with_stub(
        ["yubikey", "rotate-bws-token"],
        Some("1\nnew-token\ny\n2\nn\n"),
        &stub,
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("rotate another YubiKey? [y/N]: "));
    assert!(run.output.contains("\"serial\": 2001"));
    assert!(run.output.contains("\"serial\": 2002"));
    let final_yubikey = run.final_yubikey()?;
    assert_stored_secret(
        &final_yubikey,
        PRIMARY_SERIAL,
        "bws-access-token",
        "new-token",
    );
    assert_stored_secret(
        &final_yubikey,
        SPARE_SERIAL,
        "bws-access-token",
        "new-token",
    );
    Ok(())
}

#[test]
fn verify_yubikey_runs_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["verify-yubikey", "--serial", "2001"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"name\": \"local-storage\""));
    assert!(stdout.contains("\"status\": \"ok\""));
    assert!(stdout.contains("\"name\": \"bws\""));
    assert!(stdout.contains("\"status\": \"skipped\""));
    assert!(!run.has_bws_observation());
    Ok(())
}

#[test]
fn verify_yubikey_runs_bws_external_check() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(
        ["verify-yubikey", "--serial", "2001", "--check", "bws"],
        None,
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"name\": \"local-storage\""));
    assert!(stdout.contains("\"name\": \"bws\""));
    assert!(stdout.contains("\"status\": \"ok\""));
    let final_bws = run.final_bws()?;
    assert_eq!(
        final_bws["resolved_secrets"]["gpg-secret-key-backup"],
        json!("gpg-secret")
    );
    assert_eq!(
        final_bws["resolved_secrets"]["password-store-remote"],
        json!("https://example.invalid/repo.git")
    );
    Ok(())
}

#[test]
fn verify_yubikey_requires_serial_when_multiple_devices_are_detected() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([
            provisioned_device_spec(PRIMARY_SERIAL),
            provisioned_device_spec(SPARE_SERIAL),
        ]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["verify-yubikey"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr
            .contains("multiple YubiKeys detected; pass --serial to select a device")
    );
    Ok(())
}

#[test]
fn verify_yubikey_auto_selects_single_detected_device() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["verify-yubikey"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"serial\": 2001"));
    assert!(stdout.contains("\"name\": \"local-storage\""));
    assert!(stdout.contains("\"status\": \"ok\""));
    Ok(())
}

#[test]
fn verify_yubikey_rejects_all_with_check() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([
            provisioned_device_spec(PRIMARY_SERIAL),
            provisioned_device_spec(SPARE_SERIAL),
        ]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["verify-yubikey", "--all", "--check", "bws"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr
            .contains("--all and --check cannot be used together")
    );
    assert!(
        !run.stderr
            .contains("multiple YubiKeys detected; pass --serial to select a device"),
        "input precondition must fail before device resolution: {}",
        run.stderr
    );
    Ok(())
}

#[test]
fn put_stdin_requires_serial_before_reading_secret() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(
        ["yubikey", "put", "bws-access-token", "--stdin"],
        None,
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("pass --serial in non-interactive use"));
    Ok(())
}

#[test]
fn verify_yubikey_audits_stub_route_in_internal_stub_build() -> TestResult<()> {
    let run = run_pipe_without_stub(["verify-yubikey"], None)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr
            .contains("YubiKey internal stub spec JSON is not configured"),
        "stderr: {}",
        run.stderr
    );
    Ok(())
}

#[test]
fn verify_yubikey_requires_pin_when_device_policy_demands_it() -> TestResult<()> {
    let initial = yubikey_spec_requiring_pin([provisioned_device_spec(PRIMARY_SERIAL)]);
    let stub = StubPorts::new(initial, bws_spec());
    let run = run_pipe_with_stub(["verify-yubikey", "--serial", "2001"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr.contains("PIN") || run.stderr.contains("pin"),
        "stderr: {}",
        run.stderr
    );
    Ok(())
}

#[test]
fn put_updates_final_yubikey_spec_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([writable_bws_access_token_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let put_run = run_pipe_with_stub(
        [
            "yubikey",
            "put",
            "bws-access-token",
            "--serial",
            "2001",
            "--stdin",
        ],
        Some("new-token\r"),
        &stub,
    )?;
    assert!(put_run.success, "stderr: {}", put_run.stderr);
    assert_stored_secret(
        &put_run.final_yubikey()?,
        PRIMARY_SERIAL,
        "bws-access-token",
        "new-token\r",
    );
    Ok(())
}

#[test]
fn get_reads_seeded_secret_with_yubikey_path() -> TestResult<()> {
    let initial_device =
        seeded_device_spec(PRIMARY_SERIAL, "seed@example.com", "seed-pw", "seed-token");
    let stub = StubPorts::new(yubikey_spec([initial_device]), bws_spec());
    let run = run_pipe_with_stub(
        ["yubikey", "get", "bws-access-token", "--serial", "2001"],
        None,
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert_eq!(run.user_stdout(), "seed-token");
    Ok(())
}

#[test]
fn get_fails_when_storage_is_corrupt_with_yubikey_path() -> TestResult<()> {
    let initial_device = storage_decode_error_device_spec(
        provisioned_device_spec(PRIMARY_SERIAL),
        "bws-access-token",
    );
    let stub = StubPorts::new(yubikey_spec([initial_device]), bws_spec());
    let run = run_pipe_with_stub(
        ["yubikey", "get", "bws-access-token", "--serial", "2001"],
        None,
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to decode bws-access-token"));
    Ok(())
}

#[test]
fn rotate_fails_when_seeded_storage_is_corrupt_with_yubikey_path() -> TestResult<()> {
    let initial_device =
        storage_decode_error_device_spec(provisioned_device_spec(PRIMARY_SERIAL), "bw-password");
    let stub = StubPorts::new(yubikey_spec([initial_device]), bws_spec());
    let run = run_pipe_with_stub(
        ["yubikey", "rotate-bws-token", "--serial", "2001", "--stdin"],
        Some("new-token\r"),
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to decode bw-password"));
    Ok(())
}

#[test]
fn verify_fails_when_seeded_storage_is_corrupt_with_yubikey_path() -> TestResult<()> {
    let initial_device =
        storage_decode_error_device_spec(provisioned_device_spec(PRIMARY_SERIAL), "bw-email");
    let stub = StubPorts::new(yubikey_spec([initial_device]), bws_spec());
    let run = run_pipe_with_stub(["verify-yubikey", "--serial", "2001"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to decode bw-email"));
    Ok(())
}

fn run_pipe_with_stub<const N: usize>(
    args: [&str; N],
    input: Option<&str>,
    stub: &StubPorts,
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
    stub.apply_to_command(&mut command)?;

    let mut child = command.spawn()?;
    if let Some(input) = input {
        let mut stdin = child.stdin.take().context("failed to open child stdin")?;
        write_child_stdin(&mut stdin, input)?;
    }

    let output = child.wait_with_output()?;
    Ok(CommandRun {
        success: output.status.success(),
        stdout: String::from_utf8(output.stdout)?,
        stderr: String::from_utf8(output.stderr)?,
    })
}

fn run_pipe_without_stub<const N: usize>(
    args: [&str; N],
    input: Option<&str>,
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

    let mut child = command.spawn()?;
    if let Some(input) = input {
        let mut stdin = child.stdin.take().context("failed to open child stdin")?;
        write_child_stdin(&mut stdin, input)?;
    }

    let output = child.wait_with_output()?;
    Ok(CommandRun {
        success: output.status.success(),
        stdout: String::from_utf8(output.stdout)?,
        stderr: String::from_utf8(output.stderr)?,
    })
}

fn run_pty_with_stub<const N: usize>(
    args: [&str; N],
    input: Option<&str>,
    stub: &StubPorts,
) -> TestResult<PtyRun> {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_dotfiles"));
    command.arg("secrets");
    command.args(args);
    stub.apply_to_pty_command(&mut command)?;
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

fn write_child_stdin(mut stdin: impl Write, input: &str) -> TestResult<()> {
    match stdin.write_all(input.as_bytes()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(error.into()),
    }
}

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
            let _ = child.wait();
            anyhow::bail!("timed out waiting for PTY child process");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn yubikey_spec<const N: usize>(yubikeys: [Value; N]) -> Value {
    let yubikeys = Vec::from(yubikeys);
    json!({
        "yubikeys": yubikeys,
        "requires_pin": false
    })
}

fn yubikey_spec_requiring_pin<const N: usize>(yubikeys: [Value; N]) -> Value {
    let yubikeys = Vec::from(yubikeys);
    json!({
        "yubikeys": yubikeys,
        "requires_pin": true
    })
}

fn fresh_device_spec(serial: u32) -> Value {
    json!({
        "serial": serial,
        "fixture": "fresh"
    })
}

fn provisioned_device_spec(serial: u32) -> Value {
    json!({
        "serial": serial,
        "fixture": "provisioned"
    })
}

fn writable_bws_access_token_device_spec(serial: u32) -> Value {
    json!({
        "serial": serial,
        "fixture": "writable-bws-access-token"
    })
}

fn seeded_device_spec(
    serial: u32,
    bw_email: &str,
    bw_password: &str,
    bws_access_token: &str,
) -> Value {
    json!({
        "serial": serial,
        "fixture": "seeded",
        "bw-email": bw_email,
        "bw-password": bw_password,
        "bws-access-token": bws_access_token
    })
}

fn storage_decode_error_device_spec(mut device: Value, secret_name: &str) -> Value {
    device["storage_decode_errors"] = json!([secret_name]);
    device
}

fn bws_spec() -> Value {
    json!({
        "fixture": "default-recovery-project"
    })
}

fn assert_stored_secret(store: &Value, serial: u32, secret_name: &str, expected: &str) {
    assert_eq!(
        store["yubikeys"][serial.to_string()]["stored_secrets"][secret_name],
        json!(expected),
        "unexpected final YubiKey observed secret: serial={serial} name={secret_name}"
    );
}

fn strip_observation_lines(output: &str) -> String {
    let mut visible = String::new();
    for segment in output.split_inclusive('\n') {
        if !segment.starts_with(STUB_OBSERVATION_PREFIX) {
            visible.push_str(segment);
        }
    }
    visible
}

fn final_observation(output: &str, port: &str) -> TestResult<Value> {
    observation_frames(output)
        .filter(|frame| frame["port"] == json!(port))
        .filter_map(|frame| frame.get("observation").cloned())
        .last()
        .ok_or_else(|| anyhow::anyhow!("missing final {port} observation"))
}

fn has_observation(output: &str, port: &str) -> bool {
    observation_frames(output).any(|frame| frame["port"] == json!(port))
}

fn observation_frames(output: &str) -> impl Iterator<Item = Value> + '_ {
    output.lines().filter_map(|line| {
        let body = line
            .trim_end_matches('\r')
            .strip_prefix(STUB_OBSERVATION_PREFIX)?;
        serde_json::from_str(body).ok()
    })
}

fn bootstrap_json() -> &'static str {
    r#"{
  "bw-email": "u@example.com",
  "bw-password": "pw",
  "bws-access-token": "token"
}
"#
}
