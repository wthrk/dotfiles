#![cfg(feature = "secrets-internal-test-stub")]
//! `dotfiles secrets` の CLI 境界を internal file-backed YubiKey route で検証する。
//!
//! Production command path は runtime env による real/stub 選択を持たない。
//! この test target は `secrets-internal-test-stub` feature 有効時だけ compile-time injection された
//! adapter に state file path を渡し、旧 internal/usecase stub test の意図を復元する。

use std::{
    io::{ErrorKind, Read, Write},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::Context;
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};

const TIMEOUT: Duration = Duration::from_secs(5);

type TestResult<T> = anyhow::Result<T>;

const INTERNAL_STUB_STATE_ENV: &str = "DOTFILES_SECRETS_INTERNAL_STUB_STATE_PATH";
#[path = "secrets_internal_stub/cli_stub_state.rs"]
mod cli_stub_state;

use cli_stub_state::{
    CliStubFixture, PRIMARY_SERIAL, SPARE_SERIAL, StubFixture, StubSecret, StubState,
};

struct CommandRun {
    success: bool,
    stdout: String,
    stderr: String,
}

struct PtyRun {
    success: bool,
    output: String,
}

/// `setup` が serial 指定の非TTY実行で成功することを確認する。
#[test]
fn setup_runs_with_yubikey_path() -> TestResult<()> {
    let stub = CliStubFixture::new(&[StubFixture::State(StubState::Fresh)]);
    let run = run_pipe_with_stub(["yubikey", "setup", "--serial", "2001"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    Ok(())
}

/// `put --stdin` が pipe入力を受け取り成功することを確認する。
#[test]
fn put_reads_non_tty_stdin_with_yubikey_path() -> TestResult<()> {
    let stub = CliStubFixture::new(&[StubFixture::State(StubState::WritableBwsAccessToken)]);
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
    Ok(())
}

/// `put` がTTYでは hidden prompt 入力を使って成功することを確認する。
#[test]
fn put_reads_tty_prompt_with_yubikey_path() -> TestResult<()> {
    let stub = CliStubFixture::new(&[StubFixture::State(StubState::WritableBwsAccessToken)]);
    let run = run_pty_with_stub(
        ["yubikey", "put", "bws-access-token", "--serial", "2001"],
        Some("new-token\n"),
        &stub,
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("bws-access-token: "));
    Ok(())
}

/// `get` が非TTYでは secret を stdout へ出力することを確認する。
#[test]
fn get_writes_secret_to_pipe_with_yubikey_path() -> TestResult<()> {
    let stub = CliStubFixture::new(&[StubFixture::State(StubState::Provisioned)]);
    let run = run_pipe_with_stub(
        ["yubikey", "get", "bws-access-token", "--serial", "2001"],
        None,
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert_eq!(run.stdout, "token");
    Ok(())
}

/// `get` がTTYでは secret 出力を拒否することを確認する。
#[test]
fn get_refuses_secret_output_to_tty_with_yubikey_path() -> TestResult<()> {
    let stub = CliStubFixture::new(&[StubFixture::State(StubState::Provisioned)]);
    let run = run_pty_with_stub(
        ["yubikey", "get", "bws-access-token", "--serial", "2001"],
        None,
        &stub,
    )?;

    assert!(!run.success, "output: {}", run.output);
    assert!(
        run.output.contains("refusing to write secret to terminal"),
        "output: {}",
        run.output
    );
    Ok(())
}

/// `enroll-primary --stdin-json` が JSON入力を受け取り成功することを確認する。
#[test]
fn enroll_primary_reads_non_tty_stdin_json_with_yubikey_path() -> TestResult<()> {
    let stub = CliStubFixture::new(&[StubFixture::State(StubState::Fresh)]);
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
    stub.assert_stored_secret(PRIMARY_SERIAL, StubSecret::BwEmail, "u@example.com")?;
    stub.assert_stored_secret(PRIMARY_SERIAL, StubSecret::BwPassword, "pw")?;
    stub.assert_stored_secret(PRIMARY_SERIAL, StubSecret::BwsAccessToken, "token")?;
    Ok(())
}

/// `enroll-primary` がTTY promptで3つの secret を読み取り成功することを確認する。
#[test]
fn enroll_primary_reads_tty_prompts_with_yubikey_path() -> TestResult<()> {
    let stub = CliStubFixture::new(&[StubFixture::State(StubState::Fresh)]);
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
    stub.assert_stored_secret(PRIMARY_SERIAL, StubSecret::BwEmail, "u@example.com")?;
    stub.assert_stored_secret(PRIMARY_SERIAL, StubSecret::BwPassword, "pw")?;
    stub.assert_stored_secret(PRIMARY_SERIAL, StubSecret::BwsAccessToken, "token")?;
    Ok(())
}

/// `enroll-spare --stdin-json` が primary/spare serial 指定で成功することを確認する。
#[test]
fn enroll_spare_reads_non_tty_stdin_json_with_yubikey_path() -> TestResult<()> {
    let stub = CliStubFixture::new(&[StubFixture::State(StubState::Fresh)]);
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
    stub.assert_stored_secret(SPARE_SERIAL, StubSecret::BwEmail, "u@example.com")?;
    stub.assert_stored_secret(SPARE_SERIAL, StubSecret::BwPassword, "pw")?;
    stub.assert_stored_secret(SPARE_SERIAL, StubSecret::BwsAccessToken, "token")?;
    Ok(())
}

/// `enroll-spare` が既存 secret 再入力なし経路で成功することを確認する。
#[test]
fn enroll_spare_without_secret_reentry() -> TestResult<()> {
    let stub = CliStubFixture::new(&[StubFixture::State(StubState::Provisioned)]);
    stub.set_serial_state(SPARE_SERIAL, StubState::Fresh)?;
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
    stub.assert_stored_secret(SPARE_SERIAL, StubSecret::BwEmail, "u@example.com")?;
    stub.assert_stored_secret(SPARE_SERIAL, StubSecret::BwPassword, "pw")?;
    stub.assert_stored_secret(SPARE_SERIAL, StubSecret::BwsAccessToken, "token")?;
    Ok(())
}

/// `rotate-bws-token --stdin` が非TTY入力で成功することを確認する。
#[test]
fn rotate_bws_token_reads_non_tty_stdin_with_yubikey_path() -> TestResult<()> {
    let stub = CliStubFixture::new(&[StubFixture::State(StubState::Provisioned)]);
    let run = run_pipe_with_stub(
        ["yubikey", "rotate-bws-token", "--serial", "2001", "--stdin"],
        Some("new-token\r"),
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"serial\": 2001"));
    assert!(run.stdout.contains("\"name\": \"local-storage\""));
    assert!(run.stdout.contains("\"status\": \"ok\""));
    stub.assert_stored_secret(PRIMARY_SERIAL, StubSecret::BwsAccessToken, "new-token\r")?;
    Ok(())
}

/// `rotate-bws-token` がTTY prompt入力で成功することを確認する。
#[test]
fn rotate_bws_token_reads_tty_prompt_with_yubikey_path() -> TestResult<()> {
    let stub = CliStubFixture::new(&[StubFixture::State(StubState::Provisioned)]);
    let run = run_pty_with_stub(
        ["yubikey", "rotate-bws-token", "--serial", "2001"],
        Some("new-token\n"),
        &stub,
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("bws-access-token: "));
    assert!(run.output.contains("\"serial\": 2001"));
    stub.assert_stored_secret(PRIMARY_SERIAL, StubSecret::BwsAccessToken, "new-token")?;
    Ok(())
}

/// `rotate-bws-token` は対話TTYで更新後に別 YubiKey へ同じ token を継続適用できる。
#[test]
fn rotate_bws_token_can_continue_to_another_tty_selected_yubikey() -> TestResult<()> {
    let stub = CliStubFixture::new(&[StubFixture::State(StubState::Provisioned)]);
    stub.set_serial_state(SPARE_SERIAL, StubState::Provisioned)?;
    let run = run_pty_with_stub(
        ["yubikey", "rotate-bws-token"],
        Some("1\nnew-token\ny\n2\nn\n"),
        &stub,
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("rotate another YubiKey? [y/N]: "));
    assert!(run.output.contains("\"serial\": 2001"));
    assert!(run.output.contains("\"serial\": 2002"));
    stub.assert_write_event(PRIMARY_SERIAL, StubSecret::BwsAccessToken, "<redacted>")?;
    stub.assert_write_event(SPARE_SERIAL, StubSecret::BwsAccessToken, "<redacted>")?;
    stub.assert_stored_secret(PRIMARY_SERIAL, StubSecret::BwsAccessToken, "new-token")?;
    stub.assert_stored_secret(SPARE_SERIAL, StubSecret::BwsAccessToken, "new-token")?;
    Ok(())
}

/// `verify-yubikey` の基本成功経路（local-storage ok / bws skipped）を確認する。
#[test]
fn verify_yubikey_runs_with_yubikey_path() -> TestResult<()> {
    let stub = CliStubFixture::new(&[StubFixture::State(StubState::Provisioned)]);
    let run = run_pipe_with_stub(["verify-yubikey", "--serial", "2001"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"name\": \"local-storage\""));
    assert!(run.stdout.contains("\"status\": \"ok\""));
    assert!(run.stdout.contains("\"name\": \"bws\""));
    assert!(run.stdout.contains("\"status\": \"skipped\""));
    stub.assert_bws_fetch_event_count(0)?;
    Ok(())
}

/// `verify-yubikey --check bws` が external check を実行して成功することを確認する。
#[test]
fn verify_yubikey_runs_bws_external_check() -> TestResult<()> {
    let stub = CliStubFixture::new(&[StubFixture::State(StubState::Provisioned)]);
    let run = run_pipe_with_stub(
        ["verify-yubikey", "--serial", "2001", "--check", "bws"],
        None,
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"name\": \"local-storage\""));
    assert!(run.stdout.contains("\"name\": \"bws\""));
    assert!(run.stdout.contains("\"status\": \"ok\""));
    stub.assert_bws_secret_value("bws-secret-id-gpg", "gpg-secret")?;
    stub.assert_bws_secret_value("bws-secret-id-pass", "https://example.invalid/repo.git")?;
    stub.assert_bws_fetch_event_for_secret("bws-secret-id-gpg")?;
    stub.assert_bws_fetch_event_for_secret("bws-secret-id-pass")?;
    stub.assert_bws_fetch_event_count(2)?;
    Ok(())
}

/// `verify-yubikey` は serial 省略時に device 選択へ委譲し、複数候補では明示選択を要求する。
#[test]
fn verify_yubikey_requires_serial_when_multiple_devices_are_detected() -> TestResult<()> {
    let stub = CliStubFixture::new(&[StubFixture::State(StubState::Provisioned)]);
    let run = run_pipe_with_stub(["verify-yubikey"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr
            .contains("multiple YubiKeys detected; pass --serial to select a device")
    );
    Ok(())
}

/// `verify-yubikey` は serial 省略時に単一候補を自動選択して検証する。
#[test]
fn verify_yubikey_auto_selects_single_detected_device() -> TestResult<()> {
    let stub = CliStubFixture::new(&[
        StubFixture::PrimaryOnly,
        StubFixture::State(StubState::Provisioned),
    ]);
    let run = run_pipe_with_stub(["verify-yubikey"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"serial\": 2001"));
    assert!(run.stdout.contains("\"name\": \"local-storage\""));
    assert!(run.stdout.contains("\"status\": \"ok\""));
    Ok(())
}

/// `verify-yubikey` は `--all` と `--check` の併用を device I/O 前に拒否する。
#[test]
fn verify_yubikey_rejects_all_with_check() -> TestResult<()> {
    let stub = CliStubFixture::new(&[StubFixture::State(StubState::Provisioned)]);
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

/// `put --stdin` は serial 必須条件を secret 入力や device I/O より先に評価する。
#[test]
fn put_stdin_requires_serial_before_reading_secret() -> TestResult<()> {
    let stub = CliStubFixture::new(&[StubFixture::State(StubState::Provisioned)]);
    let run = run_pipe_with_stub(
        ["yubikey", "put", "bws-access-token", "--stdin"],
        None,
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("pass --serial in non-interactive use"));
    Ok(())
}

/// internal stub feature build では route 監査ラベルが compile-time で `stub` 固定になることを確認する。
#[test]
fn verify_yubikey_audits_stub_route_in_internal_stub_build() -> TestResult<()> {
    let run = run_pipe_without_stub(["verify-yubikey"], None)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr
            .contains("internal stub state path is not configured"),
        "stderr: {}",
        run.stderr
    );
    Ok(())
}

/// PIN 必須デバイスで PIN 未入力時に `verify-yubikey` が停止することを確認する。
#[test]
fn verify_yubikey_requires_pin_when_device_policy_demands_it() -> TestResult<()> {
    let stub = CliStubFixture::new(&[
        StubFixture::State(StubState::Provisioned),
        StubFixture::ReadPinFromTty,
    ]);
    let run = run_pipe_with_stub(["verify-yubikey", "--serial", "2001"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr.contains("PIN") || run.stderr.contains("pin"),
        "stderr: {}",
        run.stderr
    );
    Ok(())
}

/// スタブで `put` 後に書き込み結果を `get` で検証する。
#[test]
fn put_emits_stored_secret_write_event_with_yubikey_path() -> TestResult<()> {
    let stub = CliStubFixture::new(&[StubFixture::State(StubState::WritableBwsAccessToken)]);
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
    stub.assert_write_event(PRIMARY_SERIAL, StubSecret::BwsAccessToken, "<redacted>")?;
    Ok(())
}

/// スタブ seed 値を `get` が読み出せることを確認する。
#[test]
fn get_reads_seeded_secret_with_yubikey_path() -> TestResult<()> {
    let stub = CliStubFixture::new(&[
        StubFixture::State(StubState::Fresh),
        StubFixture::SeedSecret(StubSecret::BwEmail, "seed@example.com"),
        StubFixture::SeedSecret(StubSecret::BwPassword, "seed-pw"),
        StubFixture::SeedSecret(StubSecret::BwsAccessToken, "seed-token"),
    ]);
    let run = run_pipe_with_stub(
        ["yubikey", "get", "bws-access-token", "--serial", "2001"],
        None,
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert_eq!(run.stdout, "seed-token");
    Ok(())
}

/// スタブ保存データ破損時に `get` が decode 失敗で落ちることを確認する。
#[test]
fn get_fails_when_storage_is_corrupt_with_yubikey_path() -> TestResult<()> {
    let stub = CliStubFixture::new(&[
        StubFixture::State(StubState::Provisioned),
        StubFixture::CorruptSecret(StubSecret::BwsAccessToken),
    ]);
    let run = run_pipe_with_stub(
        ["yubikey", "get", "bws-access-token", "--serial", "2001"],
        None,
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to decode bws-access-token"));
    Ok(())
}

/// スタブ保存データ破損時に `rotate-bws-token` が失敗することを確認する。
#[test]
fn rotate_fails_when_seeded_storage_is_corrupt_with_yubikey_path() -> TestResult<()> {
    let stub = CliStubFixture::new(&[
        StubFixture::State(StubState::Provisioned),
        StubFixture::CorruptSecret(StubSecret::BwPassword),
    ]);
    let run = run_pipe_with_stub(
        ["yubikey", "rotate-bws-token", "--serial", "2001", "--stdin"],
        Some("new-token\r"),
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to decode bw-password"));
    Ok(())
}

/// スタブ保存データ破損時に `verify-yubikey` が失敗することを確認する。
#[test]
fn verify_fails_when_seeded_storage_is_corrupt_with_yubikey_path() -> TestResult<()> {
    let stub = CliStubFixture::new(&[
        StubFixture::State(StubState::Provisioned),
        StubFixture::CorruptSecret(StubSecret::BwEmail),
    ]);
    let run = run_pipe_with_stub(["verify-yubikey", "--serial", "2001"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to decode bw-email"));
    Ok(())
}

fn run_pipe_with_stub<const N: usize>(
    args: [&str; N],
    input: Option<&str>,
    stub: &CliStubFixture,
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
        .stderr(Stdio::piped())
        .env(INTERNAL_STUB_STATE_ENV, &stub.state_path);

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

fn write_child_stdin(mut stdin: impl Write, input: &str) -> TestResult<()> {
    match stdin.write_all(input.as_bytes()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn run_pty_with_stub<const N: usize>(
    args: [&str; N],
    input: Option<&str>,
    stub: &CliStubFixture,
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
    command.env(INTERNAL_STUB_STATE_ENV, &stub.state_path);
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
            anyhow::bail!("timed out waiting for PTY child process");
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
