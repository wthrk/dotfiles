//! `dotfiles secrets` の CLI 境界を実プロセスの TTY / pipe で検証する。
//!
//! テスト対象プロセスは production `dotfiles` を直接起動し、stdin、stdout、stderr、
//! TTY 判定、prompt 入力を実際のプロセス境界で確認する。

use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use dotfiles_cli_secrets_test_contract::{
    ADAPTER_ROUTE_AUDIT_PREFIX, CORRUPT_SECRET_ENV, PRIMARY_SERIAL, PRIMARY_STUB_STATE_ENV,
    READ_PIN_FROM_TTY_ENV, SEED_BW_EMAIL_ENV, SEED_BW_PASSWORD_ENV, SEED_BWS_ACCESS_TOKEN_ENV,
    SPARE_SERIAL, SPARE_STUB_STATE_ENV, STUB_STATE_ENV, StubSecret, StubState,
    TEST_STUB_CONTEXT_ENV, TEST_STUB_CONTEXT_VALUE, USE_TEST_STUB_ENV, format_write_event,
};
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};

type TestResult<T> = anyhow::Result<T>;

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

#[derive(Clone, Copy)]
enum StubFixture {
    State(StubState),
    #[expect(unused)]
    SerialState(u32, StubState),
    SeedSecret(StubSecret, &'static str),
    CorruptSecret(StubSecret),
    ReadPinFromTty,
}

/// `setup` が serial 指定の非TTY実行で成功することを確認する。
#[test]
fn setup_runs_with_yubikey_path() -> TestResult<()> {
    let run = run_pipe_with_stub(
        ["yubikey", "setup", "--serial", "2001"],
        None,
        &[StubFixture::State(StubState::Fresh)],
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(
        run.stderr
            .contains(&format!("{ADAPTER_ROUTE_AUDIT_PREFIX}=stub")),
        "stderr: {}",
        run.stderr
    );
    Ok(())
}

/// `put --stdin` が pipe入力を受け取り成功することを確認する。
#[test]
fn put_reads_non_tty_stdin_with_yubikey_path() -> TestResult<()> {
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
        &[StubFixture::State(StubState::WritableBwsAccessToken)],
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    Ok(())
}

/// `put` がTTYでは hidden prompt 入力を使って成功することを確認する。
#[test]
fn put_reads_tty_prompt_with_yubikey_path() -> TestResult<()> {
    let run = run_pty_with_stub(
        ["yubikey", "put", "bws-access-token", "--serial", "2001"],
        Some("new-token\n"),
        &[StubFixture::State(StubState::WritableBwsAccessToken)],
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("bws-access-token: "));
    Ok(())
}

/// `get` が非TTYでは secret を stdout へ出力することを確認する。
#[test]
fn get_writes_secret_to_pipe_with_yubikey_path() -> TestResult<()> {
    let run = run_pipe(
        ["yubikey", "get", "bws-access-token", "--serial", "2001"],
        None,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert_eq!(run.stdout, "token");
    Ok(())
}

/// `get` がTTYでは secret 出力を拒否することを確認する。
#[test]
fn get_refuses_secret_output_to_tty_with_yubikey_path() -> TestResult<()> {
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

/// `enroll-primary --stdin-json` が JSON入力を受け取り成功することを確認する。
#[test]
fn enroll_primary_reads_non_tty_stdin_json_with_yubikey_path() -> TestResult<()> {
    let run = run_pipe_with_stub(
        [
            "yubikey",
            "enroll-primary",
            "--serial",
            "2001",
            "--stdin-json",
        ],
        Some(bootstrap_json()),
        &[StubFixture::State(StubState::Fresh)],
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"role\": \"primary\""));
    assert!(run.stdout.contains("\"name\": \"local-storage\""));
    assert!(run.stdout.contains("\"status\": \"ok\""));
    Ok(())
}

/// `enroll-primary` がTTY promptで3つの secret を読み取り成功することを確認する。
#[test]
fn enroll_primary_reads_tty_prompts_with_yubikey_path() -> TestResult<()> {
    let run = run_pty_with_stub(
        ["yubikey", "enroll-primary", "--serial", "2001"],
        Some("u@example.com\npw\nnew-token\n"),
        &[StubFixture::State(StubState::Fresh)],
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("bw-email: "));
    assert!(run.output.contains("bw-password: "));
    assert!(run.output.contains("bws-access-token: "));
    assert!(run.output.contains("\"role\": \"primary\""));
    Ok(())
}

/// `enroll-spare --stdin-json` が primary/spare serial 指定で成功することを確認する。
#[test]
fn enroll_spare_reads_non_tty_stdin_json_with_yubikey_path() -> TestResult<()> {
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

/// `enroll-spare` が既存 secret 再入力なし経路で成功することを確認する。
#[test]
fn enroll_spare_without_secret_reentry() -> TestResult<()> {
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

/// `rotate-bws-token --stdin` が非TTY入力で成功することを確認する。
#[test]
fn rotate_bws_token_reads_non_tty_stdin_with_yubikey_path() -> TestResult<()> {
    let run = run_pipe(
        ["yubikey", "rotate-bws-token", "--serial", "2001", "--stdin"],
        Some("new-token\r"),
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"serial\": 2001"));
    assert!(run.stdout.contains("\"name\": \"local-storage\""));
    assert!(run.stdout.contains("\"status\": \"ok\""));
    Ok(())
}

/// `rotate-bws-token` がTTY prompt入力で成功することを確認する。
#[test]
fn rotate_bws_token_reads_tty_prompt_with_yubikey_path() -> TestResult<()> {
    let run = run_pty(
        ["yubikey", "rotate-bws-token", "--serial", "2001"],
        Some("new-token\n"),
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("bws-access-token: "));
    assert!(run.output.contains("\"serial\": 2001"));
    Ok(())
}

/// `verify-yubikey` の基本成功経路（local-storage ok / bws skipped）を確認する。
#[test]
fn verify_yubikey_runs_with_yubikey_path() -> TestResult<()> {
    let run = run_pipe(["verify-yubikey", "--serial", "2001"], None)?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"name\": \"local-storage\""));
    assert!(run.stdout.contains("\"status\": \"ok\""));
    assert!(run.stdout.contains("\"name\": \"bws\""));
    assert!(run.stdout.contains("\"status\": \"skipped\""));
    Ok(())
}

/// `verify-yubikey` が `--all` と `--check` の併用を拒否することを確認する。
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

/// 非対話 `verify-yubikey` が `--serial` 省略時に失敗することを確認する。
#[test]
fn verify_yubikey_requires_serial_in_non_interactive_use() -> TestResult<()> {
    let run = run_pipe(["verify-yubikey"], None)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("pass --serial in non-interactive use"));
    Ok(())
}

/// stub env を設定しない場合でも同じ CLI 境界が real route を監査出力することを確認する。
#[test]
fn verify_yubikey_audits_real_route_when_stub_env_is_absent() -> TestResult<()> {
    let run = run_pipe_without_stub(["verify-yubikey"], None)?;

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

/// PIN 必須デバイスで PIN 未入力時に `verify-yubikey` が停止することを確認する。
#[test]
fn verify_yubikey_requires_pin_when_device_policy_demands_it() -> TestResult<()> {
    let run = run_pipe_with_stub(
        ["verify-yubikey", "--serial", "2001"],
        None,
        &[StubFixture::ReadPinFromTty],
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr.contains("PIN") || run.stderr.contains("pin"),
        "stderr: {}",
        run.stderr
    );
    Ok(())
}

/// スタブで `put` 後に書き込みイベント（内部状態由来）を検証する。
#[test]
fn put_emits_stored_secret_write_event_with_yubikey_path() -> TestResult<()> {
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
        &[StubFixture::State(StubState::WritableBwsAccessToken)],
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert_stub_write_event(
        &run.stderr,
        PRIMARY_SERIAL,
        StubSecret::BwsAccessToken,
        "<redacted>",
    );
    Ok(())
}

/// スタブ seed 値を `get` が読み出せることを確認する。
#[test]
fn get_reads_seeded_secret_with_yubikey_path() -> TestResult<()> {
    let run = run_pipe_with_stub(
        ["yubikey", "get", "bws-access-token", "--serial", "2001"],
        None,
        &[
            StubFixture::State(StubState::Fresh),
            StubFixture::SeedSecret(StubSecret::BwEmail, "seed@example.com"),
            StubFixture::SeedSecret(StubSecret::BwPassword, "seed-pw"),
            StubFixture::SeedSecret(StubSecret::BwsAccessToken, "seed-token"),
        ],
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert_eq!(run.stdout, "seed-token");
    Ok(())
}

/// スタブ保存データ破損時に `get` が decode 失敗で落ちることを確認する。
#[test]
fn get_fails_when_storage_is_corrupt_with_yubikey_path() -> TestResult<()> {
    let run = run_pipe_with_stub(
        ["yubikey", "get", "bws-access-token", "--serial", "2001"],
        None,
        &[
            StubFixture::State(StubState::Provisioned),
            StubFixture::CorruptSecret(StubSecret::BwsAccessToken),
        ],
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to decode bws-access-token"));
    Ok(())
}

/// スタブ保存データ破損時に `rotate-bws-token` が失敗することを確認する。
#[test]
fn rotate_fails_when_seeded_storage_is_corrupt_with_yubikey_path() -> TestResult<()> {
    let run = run_pipe_with_stub(
        ["yubikey", "rotate-bws-token", "--serial", "2001", "--stdin"],
        Some("new-token\r"),
        &[
            StubFixture::State(StubState::Provisioned),
            StubFixture::CorruptSecret(StubSecret::BwPassword),
        ],
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to decode bw-password"));
    Ok(())
}

/// スタブ保存データ破損時に `verify-yubikey` が失敗することを確認する。
#[test]
fn verify_fails_when_seeded_storage_is_corrupt_with_yubikey_path() -> TestResult<()> {
    let run = run_pipe_with_stub(
        ["verify-yubikey", "--serial", "2001"],
        None,
        &[
            StubFixture::State(StubState::Provisioned),
            StubFixture::CorruptSecret(StubSecret::BwEmail),
        ],
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to decode bw-email"));
    Ok(())
}

/// 非 TTY 実行では stdin/stdout/stderr を明示的に pipe/null へ接続し、TTY 判定を実際に変える。
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
    command.env(USE_TEST_STUB_ENV, "true");
    command.env(TEST_STUB_CONTEXT_ENV, TEST_STUB_CONTEXT_VALUE);

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

fn run_pipe_with_stub<const N: usize>(
    args: [&str; N],
    input: Option<&str>,
    fixtures: &[StubFixture],
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
    command.env(USE_TEST_STUB_ENV, "true");
    command.env(TEST_STUB_CONTEXT_ENV, TEST_STUB_CONTEXT_VALUE);
    apply_stub_fixtures(&mut command, fixtures);

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
    command.env_remove(USE_TEST_STUB_ENV);
    command.env_remove(TEST_STUB_CONTEXT_ENV);

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
    command.args(args);
    command.env(USE_TEST_STUB_ENV, "true");
    command.env(TEST_STUB_CONTEXT_ENV, TEST_STUB_CONTEXT_VALUE);
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

fn run_pty_with_stub<const N: usize>(
    args: [&str; N],
    input: Option<&str>,
    fixtures: &[StubFixture],
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
    command.env(USE_TEST_STUB_ENV, "true");
    command.env(TEST_STUB_CONTEXT_ENV, TEST_STUB_CONTEXT_VALUE);
    apply_stub_fixtures_to_pty(&mut command, fixtures);
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

fn apply_stub_fixtures(command: &mut Command, fixtures: &[StubFixture]) {
    for fixture in fixtures {
        match fixture {
            StubFixture::State(state) => {
                command.env(STUB_STATE_ENV, state.env_value());
            }
            StubFixture::SerialState(serial, state) if *serial == PRIMARY_SERIAL => {
                command.env(PRIMARY_STUB_STATE_ENV, state.env_value());
            }
            StubFixture::SerialState(serial, state) if *serial == SPARE_SERIAL => {
                command.env(SPARE_STUB_STATE_ENV, state.env_value());
            }
            StubFixture::SerialState(_, _) => {}
            StubFixture::SeedSecret(StubSecret::BwEmail, value) => {
                command.env(SEED_BW_EMAIL_ENV, value);
            }
            StubFixture::SeedSecret(StubSecret::BwPassword, value) => {
                command.env(SEED_BW_PASSWORD_ENV, value);
            }
            StubFixture::SeedSecret(StubSecret::BwsAccessToken, value) => {
                command.env(SEED_BWS_ACCESS_TOKEN_ENV, value);
            }
            StubFixture::CorruptSecret(secret) => {
                command.env(CORRUPT_SECRET_ENV, secret.contract_name());
            }
            StubFixture::ReadPinFromTty => {
                command.env(READ_PIN_FROM_TTY_ENV, "true");
            }
        }
    }
}

fn apply_stub_fixtures_to_pty(command: &mut CommandBuilder, fixtures: &[StubFixture]) {
    for fixture in fixtures {
        match fixture {
            StubFixture::State(state) => {
                command.env(STUB_STATE_ENV, state.env_value());
            }
            StubFixture::SerialState(serial, state) if *serial == PRIMARY_SERIAL => {
                command.env(PRIMARY_STUB_STATE_ENV, state.env_value());
            }
            StubFixture::SerialState(serial, state) if *serial == SPARE_SERIAL => {
                command.env(SPARE_STUB_STATE_ENV, state.env_value());
            }
            StubFixture::SerialState(_, _) => {}
            StubFixture::SeedSecret(StubSecret::BwEmail, value) => {
                command.env(SEED_BW_EMAIL_ENV, value);
            }
            StubFixture::SeedSecret(StubSecret::BwPassword, value) => {
                command.env(SEED_BW_PASSWORD_ENV, value);
            }
            StubFixture::SeedSecret(StubSecret::BwsAccessToken, value) => {
                command.env(SEED_BWS_ACCESS_TOKEN_ENV, value);
            }
            StubFixture::CorruptSecret(secret) => {
                command.env(CORRUPT_SECRET_ENV, secret.contract_name());
            }
            StubFixture::ReadPinFromTty => {
                command.env(READ_PIN_FROM_TTY_ENV, "true");
            }
        }
    }
}

fn assert_stub_write_event(output: &str, serial: u32, secret: StubSecret, value: &str) {
    let expected = format_write_event(serial, secret.contract_name(), value);
    assert!(
        output.contains(&expected),
        "missing write event: {expected}\n{output}"
    );
}
