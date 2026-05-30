#![cfg(feature = "secrets-internal-test-stub")]
//! `dotfiles secrets` の CLI 境界を feature-gated internal backend stub で検証する。
//!
//! Production command path は runtime env による real/stub 選択を持たない。この test target は
//! port ごとの初期 datastore JSON を env で渡し、CLI 実行後に port ごとの最終 datastore JSON だけを
//! 観測する。

use std::{
    fs,
    io::{ErrorKind, Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

use anyhow::Context;
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};
use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(15);
const PRIMARY_SERIAL: u32 = 2001;
const SPARE_SERIAL: u32 = 2002;
const MANIFEST_OBJECT_ID: &str = "6291222";

const YUBIKEY_STUB_DATASTORE_ENV: &str = "DOTFILES_SECRETS_YUBIKEY_STUB_DATASTORE_JSON";
const YUBIKEY_STUB_OUTPUT_ENV: &str = "DOTFILES_SECRETS_YUBIKEY_STUB_OUTPUT_PATH";
const BWS_STUB_DATASTORE_ENV: &str = "DOTFILES_SECRETS_BWS_STUB_DATASTORE_JSON";
const BWS_STUB_OUTPUT_ENV: &str = "DOTFILES_SECRETS_BWS_STUB_OUTPUT_PATH";
static STUB_DATASTORE_SEQ: AtomicU64 = AtomicU64::new(1);

type TestResult<T> = anyhow::Result<T>;

struct CommandRun {
    success: bool,
    stdout: String,
    stderr: String,
}

struct PtyRun {
    success: bool,
    output: String,
}

struct StubDatastores {
    yubikey_initial: Value,
    bws_initial: Value,
    yubikey_output_path: PathBuf,
    bws_output_path: PathBuf,
}

impl StubDatastores {
    fn new(yubikey_initial: Value, bws_initial: Value) -> Self {
        let unique = format!(
            "dotfiles-secrets-stub-{}-{}-{}",
            std::process::id(),
            STUB_DATASTORE_SEQ.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        );
        let dir = std::env::temp_dir();
        Self {
            yubikey_initial,
            bws_initial,
            yubikey_output_path: dir.join(format!("{unique}-yubikey.json")),
            bws_output_path: dir.join(format!("{unique}-bws.json")),
        }
    }

    fn final_yubikey(&self) -> TestResult<Value> {
        read_json_file(&self.yubikey_output_path)
    }

    fn final_bws(&self) -> TestResult<Value> {
        read_json_file(&self.bws_output_path)
    }
}

#[test]
fn setup_runs_with_yubikey_path() -> TestResult<()> {
    let stub = StubDatastores::new(
        yubikey_datastore([fresh_device(PRIMARY_SERIAL)]),
        bws_datastore(),
    );
    let run = run_pipe_with_stub(["yubikey", "setup", "--serial", "2001"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    Ok(())
}

#[test]
fn put_reads_non_tty_stdin_with_yubikey_path() -> TestResult<()> {
    let stub = StubDatastores::new(
        yubikey_datastore([writable_bws_access_token_device(PRIMARY_SERIAL)]),
        bws_datastore(),
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
        &stub.final_yubikey()?,
        PRIMARY_SERIAL,
        "bws-access-token",
        "new-token\r",
    );
    Ok(())
}

#[test]
fn put_reads_tty_prompt_with_yubikey_path() -> TestResult<()> {
    let stub = StubDatastores::new(
        yubikey_datastore([writable_bws_access_token_device(PRIMARY_SERIAL)]),
        bws_datastore(),
    );
    let run = run_pty_with_stub(
        ["yubikey", "put", "bws-access-token", "--serial", "2001"],
        Some("new-token\n"),
        &stub,
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("bws-access-token: "));
    assert_stored_secret(
        &stub.final_yubikey()?,
        PRIMARY_SERIAL,
        "bws-access-token",
        "new-token",
    );
    Ok(())
}

#[test]
fn get_writes_secret_to_pipe_with_yubikey_path() -> TestResult<()> {
    let stub = StubDatastores::new(
        yubikey_datastore([provisioned_device(PRIMARY_SERIAL)]),
        bws_datastore(),
    );
    let run = run_pipe_with_stub(
        ["yubikey", "get", "bws-access-token", "--serial", "2001"],
        None,
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert_eq!(run.stdout, "token");
    Ok(())
}

#[test]
fn get_refuses_secret_output_to_tty_with_yubikey_path() -> TestResult<()> {
    let stub = StubDatastores::new(
        yubikey_datastore([provisioned_device(PRIMARY_SERIAL)]),
        bws_datastore(),
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
    let stub = StubDatastores::new(
        yubikey_datastore([fresh_device(PRIMARY_SERIAL)]),
        bws_datastore(),
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
    assert!(run.stdout.contains("\"role\": \"primary\""));
    assert!(run.stdout.contains("\"name\": \"local-storage\""));
    assert!(run.stdout.contains("\"status\": \"ok\""));
    let final_yubikey = stub.final_yubikey()?;
    assert_stored_secret(&final_yubikey, PRIMARY_SERIAL, "bw-email", "u@example.com");
    assert_stored_secret(&final_yubikey, PRIMARY_SERIAL, "bw-password", "pw");
    assert_stored_secret(&final_yubikey, PRIMARY_SERIAL, "bws-access-token", "token");
    Ok(())
}

#[test]
fn enroll_primary_reads_tty_prompts_with_yubikey_path() -> TestResult<()> {
    let stub = StubDatastores::new(
        yubikey_datastore([fresh_device(PRIMARY_SERIAL)]),
        bws_datastore(),
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
    let final_yubikey = stub.final_yubikey()?;
    assert_stored_secret(&final_yubikey, PRIMARY_SERIAL, "bw-email", "u@example.com");
    assert_stored_secret(&final_yubikey, PRIMARY_SERIAL, "bw-password", "pw");
    assert_stored_secret(&final_yubikey, PRIMARY_SERIAL, "bws-access-token", "token");
    Ok(())
}

#[test]
fn enroll_spare_reads_non_tty_stdin_json_with_yubikey_path() -> TestResult<()> {
    let stub = StubDatastores::new(
        yubikey_datastore([fresh_device(PRIMARY_SERIAL), fresh_device(SPARE_SERIAL)]),
        bws_datastore(),
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
    assert!(run.stdout.contains("\"role\": \"spare\""));
    assert!(run.stdout.contains("\"serial\": 2002"));
    let final_yubikey = stub.final_yubikey()?;
    assert_stored_secret(&final_yubikey, SPARE_SERIAL, "bw-email", "u@example.com");
    assert_stored_secret(&final_yubikey, SPARE_SERIAL, "bw-password", "pw");
    assert_stored_secret(&final_yubikey, SPARE_SERIAL, "bws-access-token", "token");
    Ok(())
}

#[test]
fn enroll_spare_without_secret_reentry() -> TestResult<()> {
    let stub = StubDatastores::new(
        yubikey_datastore([
            provisioned_device(PRIMARY_SERIAL),
            fresh_device(SPARE_SERIAL),
        ]),
        bws_datastore(),
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
    assert!(run.stdout.contains("\"role\": \"spare\""));
    let final_yubikey = stub.final_yubikey()?;
    assert_stored_secret(&final_yubikey, SPARE_SERIAL, "bw-email", "u@example.com");
    assert_stored_secret(&final_yubikey, SPARE_SERIAL, "bw-password", "pw");
    assert_stored_secret(&final_yubikey, SPARE_SERIAL, "bws-access-token", "token");
    Ok(())
}

#[test]
fn rotate_bws_token_reads_non_tty_stdin_with_yubikey_path() -> TestResult<()> {
    let stub = StubDatastores::new(
        yubikey_datastore([provisioned_device(PRIMARY_SERIAL)]),
        bws_datastore(),
    );
    let run = run_pipe_with_stub(
        ["yubikey", "rotate-bws-token", "--serial", "2001", "--stdin"],
        Some("new-token\r"),
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"serial\": 2001"));
    assert!(run.stdout.contains("\"name\": \"local-storage\""));
    assert!(run.stdout.contains("\"status\": \"ok\""));
    assert_stored_secret(
        &stub.final_yubikey()?,
        PRIMARY_SERIAL,
        "bws-access-token",
        "new-token\r",
    );
    Ok(())
}

#[test]
fn rotate_bws_token_reads_tty_prompt_with_yubikey_path() -> TestResult<()> {
    let stub = StubDatastores::new(
        yubikey_datastore([provisioned_device(PRIMARY_SERIAL)]),
        bws_datastore(),
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
        &stub.final_yubikey()?,
        PRIMARY_SERIAL,
        "bws-access-token",
        "new-token",
    );
    Ok(())
}

#[test]
fn rotate_bws_token_can_continue_to_another_tty_selected_yubikey() -> TestResult<()> {
    let stub = StubDatastores::new(
        yubikey_datastore([
            provisioned_device(PRIMARY_SERIAL),
            provisioned_device(SPARE_SERIAL),
        ]),
        bws_datastore(),
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
    let final_yubikey = stub.final_yubikey()?;
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
    let stub = StubDatastores::new(
        yubikey_datastore([provisioned_device(PRIMARY_SERIAL)]),
        bws_datastore(),
    );
    let run = run_pipe_with_stub(["verify-yubikey", "--serial", "2001"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"name\": \"local-storage\""));
    assert!(run.stdout.contains("\"status\": \"ok\""));
    assert!(run.stdout.contains("\"name\": \"bws\""));
    assert!(run.stdout.contains("\"status\": \"skipped\""));
    assert!(!stub.bws_output_path.exists());
    Ok(())
}

#[test]
fn verify_yubikey_runs_bws_external_check() -> TestResult<()> {
    let stub = StubDatastores::new(
        yubikey_datastore([provisioned_device(PRIMARY_SERIAL)]),
        bws_datastore(),
    );
    let run = run_pipe_with_stub(
        ["verify-yubikey", "--serial", "2001", "--check", "bws"],
        None,
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"name\": \"local-storage\""));
    assert!(run.stdout.contains("\"name\": \"bws\""));
    assert!(run.stdout.contains("\"status\": \"ok\""));
    let final_bws = stub.final_bws()?;
    assert_eq!(
        final_bws["secret_values"]["bws-secret-id-gpg"],
        json!("gpg-secret")
    );
    assert_eq!(
        final_bws["secret_values"]["bws-secret-id-pass"],
        json!("https://example.invalid/repo.git")
    );
    Ok(())
}

#[test]
fn verify_yubikey_requires_serial_when_multiple_devices_are_detected() -> TestResult<()> {
    let stub = StubDatastores::new(
        yubikey_datastore([
            provisioned_device(PRIMARY_SERIAL),
            provisioned_device(SPARE_SERIAL),
        ]),
        bws_datastore(),
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
    let stub = StubDatastores::new(
        yubikey_datastore([provisioned_device(PRIMARY_SERIAL)]),
        bws_datastore(),
    );
    let run = run_pipe_with_stub(["verify-yubikey"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"serial\": 2001"));
    assert!(run.stdout.contains("\"name\": \"local-storage\""));
    assert!(run.stdout.contains("\"status\": \"ok\""));
    Ok(())
}

#[test]
fn verify_yubikey_rejects_all_with_check() -> TestResult<()> {
    let stub = StubDatastores::new(
        yubikey_datastore([
            provisioned_device(PRIMARY_SERIAL),
            provisioned_device(SPARE_SERIAL),
        ]),
        bws_datastore(),
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
    let stub = StubDatastores::new(
        yubikey_datastore([provisioned_device(PRIMARY_SERIAL)]),
        bws_datastore(),
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
            .contains("YubiKey internal stub output path is not configured"),
        "stderr: {}",
        run.stderr
    );
    Ok(())
}

#[test]
fn verify_yubikey_requires_pin_when_device_policy_demands_it() -> TestResult<()> {
    let mut initial = yubikey_datastore([provisioned_device(PRIMARY_SERIAL)]);
    initial["requires_pin"] = json!(true);
    let stub = StubDatastores::new(initial, bws_datastore());
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
fn put_updates_final_yubikey_datastore_with_yubikey_path() -> TestResult<()> {
    let stub = StubDatastores::new(
        yubikey_datastore([writable_bws_access_token_device(PRIMARY_SERIAL)]),
        bws_datastore(),
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
        &stub.final_yubikey()?,
        PRIMARY_SERIAL,
        "bws-access-token",
        "new-token\r",
    );
    Ok(())
}

#[test]
fn get_reads_seeded_secret_with_yubikey_path() -> TestResult<()> {
    let mut initial_device = fresh_device(PRIMARY_SERIAL);
    initial_device["key_exists"] = json!(true);
    initial_device["objects"][MANIFEST_OBJECT_ID] = manifest_bytes_json();
    initial_device["secrets"] = json!({
        "bw-email": "seed@example.com",
        "bw-password": "seed-pw",
        "bws-access-token": "seed-token"
    });
    let stub = StubDatastores::new(yubikey_datastore([initial_device]), bws_datastore());
    let run = run_pipe_with_stub(
        ["yubikey", "get", "bws-access-token", "--serial", "2001"],
        None,
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert_eq!(run.stdout, "seed-token");
    Ok(())
}

#[test]
fn get_fails_when_storage_is_corrupt_with_yubikey_path() -> TestResult<()> {
    let mut initial_device = provisioned_device(PRIMARY_SERIAL);
    initial_device["corrupt"] = json!(["bws-access-token"]);
    let stub = StubDatastores::new(yubikey_datastore([initial_device]), bws_datastore());
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
    let mut initial_device = provisioned_device(PRIMARY_SERIAL);
    initial_device["corrupt"] = json!(["bw-password"]);
    let stub = StubDatastores::new(yubikey_datastore([initial_device]), bws_datastore());
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
    let mut initial_device = provisioned_device(PRIMARY_SERIAL);
    initial_device["corrupt"] = json!(["bw-email"]);
    let stub = StubDatastores::new(yubikey_datastore([initial_device]), bws_datastore());
    let run = run_pipe_with_stub(["verify-yubikey", "--serial", "2001"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to decode bw-email"));
    Ok(())
}

fn run_pipe_with_stub<const N: usize>(
    args: [&str; N],
    input: Option<&str>,
    stub: &StubDatastores,
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
    apply_stub_env(&mut command, stub)?;

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
    stub: &StubDatastores,
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
    command.env(
        YUBIKEY_STUB_DATASTORE_ENV,
        serde_json::to_string(&stub.yubikey_initial)?,
    );
    command.env(
        YUBIKEY_STUB_OUTPUT_ENV,
        stub.yubikey_output_path.to_string_lossy().as_ref(),
    );
    command.env(
        BWS_STUB_DATASTORE_ENV,
        serde_json::to_string(&stub.bws_initial)?,
    );
    command.env(
        BWS_STUB_OUTPUT_ENV,
        stub.bws_output_path.to_string_lossy().as_ref(),
    );
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

fn apply_stub_env(command: &mut Command, stub: &StubDatastores) -> TestResult<()> {
    command
        .env(
            YUBIKEY_STUB_DATASTORE_ENV,
            serde_json::to_string(&stub.yubikey_initial)?,
        )
        .env(YUBIKEY_STUB_OUTPUT_ENV, &stub.yubikey_output_path)
        .env(
            BWS_STUB_DATASTORE_ENV,
            serde_json::to_string(&stub.bws_initial)?,
        )
        .env(BWS_STUB_OUTPUT_ENV, &stub.bws_output_path);
    Ok(())
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

fn yubikey_datastore<const N: usize>(devices: [Value; N]) -> Value {
    let mut map = serde_json::Map::new();
    for device in devices {
        let serial = device["serial"]
            .as_u64()
            .expect("stub datastore serial must be u64");
        let mut device = device;
        device
            .as_object_mut()
            .expect("stub device must be object")
            .remove("serial");
        map.insert(serial.to_string(), device);
    }
    json!({
        "devices": map,
        "requires_pin": false
    })
}

fn fresh_device(serial: u32) -> Value {
    json!({
        "serial": serial,
        "key_exists": false,
        "objects": {},
        "secrets": {},
        "corrupt": []
    })
}

fn provisioned_device(serial: u32) -> Value {
    json!({
        "serial": serial,
        "key_exists": true,
        "objects": {
            "6291222": manifest_bytes_json()
        },
        "secrets": {
            "bw-email": "u@example.com",
            "bw-password": "pw",
            "bws-access-token": "token"
        },
        "corrupt": []
    })
}

fn writable_bws_access_token_device(serial: u32) -> Value {
    json!({
        "serial": serial,
        "key_exists": true,
        "objects": {
            "6291222": manifest_bytes_json()
        },
        "secrets": {
            "bw-email": "u@example.com",
            "bw-password": "pw"
        },
        "corrupt": []
    })
}

fn bws_datastore() -> Value {
    json!({
        "projects": {
            "bws-project-id-dotfiles": "dotfiles-secret-recovery"
        },
        "project_secrets": {
            "bws-project-id-dotfiles": {
                "bws-secret-id-gpg": "gpg-secret-key-backup",
                "bws-secret-id-pass": "password-store-remote"
            }
        },
        "secret_values": {
            "bws-secret-id-access-token": "token",
            "bws-secret-id-gpg": "gpg-secret",
            "bws-secret-id-pass": "https://example.invalid/repo.git"
        }
    })
}

fn assert_stored_secret(store: &Value, serial: u32, secret_name: &str, expected: &str) {
    assert_eq!(
        store["devices"][serial.to_string()]["secrets"][secret_name],
        json!(expected),
        "unexpected final YubiKey datastore secret: serial={serial} name={secret_name}"
    );
}

fn read_json_file(path: &PathBuf) -> TestResult<Value> {
    let body = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(serde_json::from_slice(&body)?)
}

fn manifest_bytes_json() -> Value {
    json!(br#"{"version":1,"app":"dotfiles.secret-recovery"}"#.to_vec())
}

fn bootstrap_json() -> &'static str {
    r#"{
  "bw-email": "u@example.com",
  "bw-password": "pw",
  "bws-access-token": "token"
}
"#
}
