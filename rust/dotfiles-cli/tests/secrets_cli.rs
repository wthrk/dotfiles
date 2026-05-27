#![cfg(feature = "secrets-internal-test-stub")]
//! `dotfiles secrets` の CLI 境界を internal mockito-backed YubiKey route で検証する。
//!
//! Production command path は runtime env による real/stub 選択を持たない。
//! この test target は `secrets-internal-test-stub` feature 有効時だけ compile-time injection された
//! adapter に mockito endpoint を渡し、旧 internal/usecase stub test の意図を復元する。

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use anyhow::Context;
use mockito::{Matcher, Server, ServerGuard};
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};

const TIMEOUT: Duration = Duration::from_secs(5);

type TestResult<T> = anyhow::Result<T>;

const ADAPTER_ROUTE_AUDIT_PREFIX: &str = "DOTFILES_SECRETS_DEVICE_ADAPTER_ROUTE";
const INTERNAL_STUB_ENDPOINT_ENV: &str = "DOTFILES_SECRETS_INTERNAL_STUB_MOCKITO_URL";
const PRIMARY_SERIAL: u32 = 2001;
const SPARE_SERIAL: u32 = 2002;
const MANIFEST_OBJECT_ID: u32 = 0x005f_ff16;
const BW_EMAIL_OBJECT_ID: u32 = 0x005f_ff17;
const BW_PASSWORD_OBJECT_ID: u32 = 0x005f_ff18;
const BWS_ACCESS_TOKEN_OBJECT_ID: u32 = 0x005f_ff19;
const MANIFEST_BYTES: &[u8] = br#"{"version":1,"app":"dotfiles.secret-recovery"}"#;

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
enum StubState {
    Fresh,
    Initialized,
    Provisioned,
    WritableBwsAccessToken,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum StubSecret {
    BwEmail,
    BwPassword,
    BwsAccessToken,
}

#[derive(Clone, Copy)]
enum StubFixture {
    State(StubState),
    SeedSecret(StubSecret, &'static str),
    CorruptSecret(StubSecret),
    ReadPinFromTty,
}

struct StubServer {
    server: ServerGuard,
    state: Arc<Mutex<StubDeviceState>>,
    _get: mockito::Mock,
    _post: mockito::Mock,
    _put: mockito::Mock,
}

struct StubDeviceState {
    key_exists: BTreeMap<u32, bool>,
    objects: BTreeMap<(u32, u32), Vec<u8>>,
    plaintexts: BTreeMap<(u32, u8), Vec<u8>>,
    corrupt: BTreeSet<(u32, u8)>,
    requires_pin: bool,
    write_events: Vec<String>,
}

/// `setup` が serial 指定の非TTY実行で成功することを確認する。
#[test]
fn setup_runs_with_yubikey_path() -> TestResult<()> {
    let stub = StubServer::new(&[StubFixture::State(StubState::Fresh)]);
    let run = run_pipe_with_stub(["yubikey", "setup", "--serial", "2001"], None, &stub)?;

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
    let stub = StubServer::new(&[StubFixture::State(StubState::WritableBwsAccessToken)]);
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
    let stub = StubServer::new(&[StubFixture::State(StubState::WritableBwsAccessToken)]);
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
    let stub = StubServer::new(&[StubFixture::State(StubState::Provisioned)]);
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
    let stub = StubServer::new(&[StubFixture::State(StubState::Provisioned)]);
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
    let stub = StubServer::new(&[StubFixture::State(StubState::Fresh)]);
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
    Ok(())
}

/// `enroll-primary` がTTY promptで3つの secret を読み取り成功することを確認する。
#[test]
fn enroll_primary_reads_tty_prompts_with_yubikey_path() -> TestResult<()> {
    let stub = StubServer::new(&[StubFixture::State(StubState::Fresh)]);
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
    Ok(())
}

/// `enroll-spare --stdin-json` が primary/spare serial 指定で成功することを確認する。
#[test]
fn enroll_spare_reads_non_tty_stdin_json_with_yubikey_path() -> TestResult<()> {
    let stub = StubServer::new(&[StubFixture::State(StubState::Fresh)]);
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
    Ok(())
}

/// `enroll-spare` が既存 secret 再入力なし経路で成功することを確認する。
#[test]
fn enroll_spare_without_secret_reentry() -> TestResult<()> {
    let stub = StubServer::new(&[StubFixture::State(StubState::Provisioned)]);
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
    Ok(())
}

/// `rotate-bws-token --stdin` が非TTY入力で成功することを確認する。
#[test]
fn rotate_bws_token_reads_non_tty_stdin_with_yubikey_path() -> TestResult<()> {
    let stub = StubServer::new(&[StubFixture::State(StubState::WritableBwsAccessToken)]);
    let run = run_pipe_with_stub(
        ["yubikey", "rotate-bws-token", "--serial", "2001", "--stdin"],
        Some("new-token\r"),
        &stub,
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
    let stub = StubServer::new(&[StubFixture::State(StubState::WritableBwsAccessToken)]);
    let run = run_pty_with_stub(
        ["yubikey", "rotate-bws-token", "--serial", "2001"],
        Some("new-token\n"),
        &stub,
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("bws-access-token: "));
    assert!(run.output.contains("\"serial\": 2001"));
    Ok(())
}

/// `verify-yubikey` の基本成功経路（local-storage ok / bws skipped）を確認する。
#[test]
fn verify_yubikey_runs_with_yubikey_path() -> TestResult<()> {
    let stub = StubServer::new(&[StubFixture::State(StubState::Provisioned)]);
    let run = run_pipe_with_stub(["verify-yubikey", "--serial", "2001"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"name\": \"local-storage\""));
    assert!(run.stdout.contains("\"status\": \"ok\""));
    assert!(run.stdout.contains("\"name\": \"bws\""));
    assert!(run.stdout.contains("\"status\": \"skipped\""));
    Ok(())
}

/// `verify-yubikey` は非対話実行で serial 省略時に device I/O へ進まず失敗する。
#[test]
fn verify_yubikey_requires_serial_in_non_interactive_use() -> TestResult<()> {
    let stub = StubServer::new(&[StubFixture::State(StubState::Provisioned)]);
    let run = run_pipe_with_stub(["verify-yubikey"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("pass --serial in non-interactive use"));
    Ok(())
}

/// `verify-yubikey` は `--all` と `--check` の併用を device I/O 前に拒否する。
#[test]
fn verify_yubikey_rejects_all_with_check() -> TestResult<()> {
    let stub = StubServer::new(&[StubFixture::State(StubState::Provisioned)]);
    let run = run_pipe_with_stub(
        [
            "verify-yubikey",
            "--serial",
            "2001",
            "--all",
            "--check",
            "bws",
        ],
        None,
        &stub,
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
    let stub = StubServer::new(&[StubFixture::State(StubState::Provisioned)]);
    let run = run_pipe_with_stub(
        ["yubikey", "put", "bws-access-token", "--stdin"],
        None,
        &stub,
    )?;

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
    let stub = StubServer::new(&[
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
    let stub = StubServer::new(&[StubFixture::State(StubState::WritableBwsAccessToken)]);
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
    let stub = StubServer::new(&[
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
    let stub = StubServer::new(&[
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
    let stub = StubServer::new(&[
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
    let stub = StubServer::new(&[
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
    stub: &StubServer,
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
        .env(INTERNAL_STUB_ENDPOINT_ENV, stub.url());

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

fn run_pty_with_stub<const N: usize>(
    args: [&str; N],
    input: Option<&str>,
    stub: &StubServer,
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
    command.env(INTERNAL_STUB_ENDPOINT_ENV, stub.url());
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

impl StubSecret {
    fn object_id(self) -> u32 {
        match self {
            Self::BwEmail => BW_EMAIL_OBJECT_ID,
            Self::BwPassword => BW_PASSWORD_OBJECT_ID,
            Self::BwsAccessToken => BWS_ACCESS_TOKEN_OBJECT_ID,
        }
    }

    fn secret_id(self) -> u8 {
        match self {
            Self::BwEmail => 1,
            Self::BwPassword => 2,
            Self::BwsAccessToken => 3,
        }
    }

    fn default_value(self) -> &'static [u8] {
        match self {
            Self::BwEmail => b"u@example.com",
            Self::BwPassword => b"pw",
            Self::BwsAccessToken => b"token",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::BwEmail => "bw-email",
            Self::BwPassword => "bw-password",
            Self::BwsAccessToken => "bws-access-token",
        }
    }

    fn from_secret_id(secret_id: u8) -> Option<Self> {
        match secret_id {
            1 => Some(Self::BwEmail),
            2 => Some(Self::BwPassword),
            3 => Some(Self::BwsAccessToken),
            _ => None,
        }
    }
}

impl StubDeviceState {
    fn new(fixtures: &[StubFixture]) -> Self {
        let mut state = Self::fresh_for_all();
        for fixture in fixtures {
            match *fixture {
                StubFixture::State(stub_state) => state.apply_state(PRIMARY_SERIAL, stub_state),
                StubFixture::SeedSecret(secret, value) => {
                    state.key_exists.insert(PRIMARY_SERIAL, true);
                    state.objects.insert(
                        (PRIMARY_SERIAL, MANIFEST_OBJECT_ID),
                        MANIFEST_BYTES.to_vec(),
                    );
                    state.objects.insert(
                        (PRIMARY_SERIAL, secret.object_id()),
                        encoded_object(secret.secret_id()),
                    );
                    state.plaintexts.insert(
                        (PRIMARY_SERIAL, secret.secret_id()),
                        value.as_bytes().to_vec(),
                    );
                }
                StubFixture::CorruptSecret(secret) => {
                    state.corrupt.insert((PRIMARY_SERIAL, secret.secret_id()));
                }
                StubFixture::ReadPinFromTty => state.requires_pin = true,
            }
        }
        state
    }

    fn fresh_for_all() -> Self {
        let mut state = Self {
            key_exists: BTreeMap::new(),
            objects: BTreeMap::new(),
            plaintexts: BTreeMap::new(),
            corrupt: BTreeSet::new(),
            requires_pin: false,
            write_events: Vec::new(),
        };
        state.apply_state(PRIMARY_SERIAL, StubState::Fresh);
        state.apply_state(SPARE_SERIAL, StubState::Fresh);
        state
    }

    fn apply_state(&mut self, serial: u32, state: StubState) {
        self.objects
            .retain(|(object_serial, _), _| *object_serial != serial);
        self.plaintexts
            .retain(|(plain_serial, _), _| *plain_serial != serial);
        match state {
            StubState::Fresh => {
                self.key_exists.insert(serial, false);
            }
            StubState::Initialized => {
                self.key_exists.insert(serial, true);
                self.objects
                    .insert((serial, MANIFEST_OBJECT_ID), MANIFEST_BYTES.to_vec());
            }
            StubState::Provisioned => {
                self.apply_state(serial, StubState::Initialized);
                for secret in [
                    StubSecret::BwEmail,
                    StubSecret::BwPassword,
                    StubSecret::BwsAccessToken,
                ] {
                    self.objects.insert(
                        (serial, secret.object_id()),
                        encoded_object(secret.secret_id()),
                    );
                    self.plaintexts.insert(
                        (serial, secret.secret_id()),
                        secret.default_value().to_vec(),
                    );
                }
            }
            StubState::WritableBwsAccessToken => {
                self.apply_state(serial, StubState::Provisioned);
                self.objects.remove(&(serial, BWS_ACCESS_TOKEN_OBJECT_ID));
                self.plaintexts
                    .remove(&(serial, StubSecret::BwsAccessToken.secret_id()));
            }
        }
    }

    fn get_status(&self, path: &str) -> usize {
        if path == "/devices" {
            return 200;
        }
        if path.ends_with("/key-exists")
            || path.ends_with("/piv-version")
            || path.ends_with("/pin-retries")
            || path.ends_with("/requires-pin")
        {
            return 200;
        }
        if let Some((serial, object_id)) = parse_object_path(path) {
            if self.objects.contains_key(&(serial, object_id)) {
                return 200;
            }
            return 404;
        }
        404
    }

    fn get_body(&self, path: &str) -> Vec<u8> {
        if path == "/devices" {
            return format!(
                r#"[{{"serial":{PRIMARY_SERIAL},"label":"stub-yubikey-{PRIMARY_SERIAL}"}},{{"serial":{SPARE_SERIAL},"label":"stub-yubikey-{SPARE_SERIAL}"}}]"#
            )
            .into_bytes();
        }
        if let Some(serial) = parse_device_suffix(path, "key-exists") {
            return format!(
                r#"{{"value":{}}}"#,
                self.key_exists.get(&serial).copied().unwrap_or(false)
            )
            .into_bytes();
        }
        if path.ends_with("/piv-version") {
            return br#"{"major":5,"minor":3,"patch":0}"#.to_vec();
        }
        if path.ends_with("/pin-retries") {
            return br#"{"value":1}"#.to_vec();
        }
        if path.ends_with("/requires-pin") {
            return format!(r#"{{"value":{}}}"#, self.requires_pin).into_bytes();
        }
        if let Some((serial, object_id)) = parse_object_path(path) {
            return self
                .objects
                .get(&(serial, object_id))
                .cloned()
                .unwrap_or_default();
        }
        Vec::new()
    }

    fn post_status(&self, path: &str) -> usize {
        if let Some((serial, secret_id)) = parse_storage_suffix(path, "open") {
            if self.corrupt.contains(&(serial, secret_id)) {
                return 500;
            }
            if self.plaintexts.contains_key(&(serial, secret_id)) {
                return 200;
            }
            return 404;
        }
        200
    }

    fn post_body(&mut self, path: &str, body: &[u8]) -> Vec<u8> {
        if let Some((serial, secret_id)) = parse_storage_suffix(path, "seal") {
            self.key_exists.insert(serial, true);
            self.plaintexts.insert((serial, secret_id), body.to_vec());
            if let Some(secret) = StubSecret::from_secret_id(secret_id) {
                self.write_events
                    .push(format_write_event(serial, secret.name(), "<redacted>"));
            }
            return encoded_object(secret_id);
        }
        if let Some((serial, secret_id)) = parse_storage_suffix(path, "open") {
            if self.corrupt.contains(&(serial, secret_id)) {
                let name = StubSecret::from_secret_id(secret_id)
                    .map(StubSecret::name)
                    .unwrap_or("unknown");
                return format!("corrupt {name}").into_bytes();
            }
            return self
                .plaintexts
                .get(&(serial, secret_id))
                .cloned()
                .unwrap_or_default();
        }
        if let Some(serial) = parse_device_suffix(path, "generate-key") {
            self.key_exists.insert(serial, true);
        }
        Vec::new()
    }

    fn put_status(&mut self, path: &str, body: &[u8]) -> usize {
        if let Some((serial, object_id)) = parse_object_path(path) {
            self.objects.insert((serial, object_id), body.to_vec());
            return 200;
        }
        404
    }
}

impl StubServer {
    fn new(fixtures: &[StubFixture]) -> Self {
        let mut server = Server::new();
        let state = Arc::new(Mutex::new(StubDeviceState::new(fixtures)));
        let get_status_state = Arc::clone(&state);
        let get_body_state = Arc::clone(&state);
        let post_status_state = Arc::clone(&state);
        let post_body_state = Arc::clone(&state);
        let put_status_state = Arc::clone(&state);

        let get = server
            .mock("GET", Matcher::Any)
            .with_status_code_from_request(move |request| {
                get_status_state
                    .lock()
                    .map(|state| state.get_status(request.path()))
                    .unwrap_or(500)
            })
            .with_body_from_request(move |request| {
                get_body_state
                    .lock()
                    .map(|state| state.get_body(request.path()))
                    .unwrap_or_default()
            })
            .expect_at_least(0)
            .create();
        let post = server
            .mock("POST", Matcher::Any)
            .with_status_code_from_request(move |request| {
                post_status_state
                    .lock()
                    .map(|state| state.post_status(request.path()))
                    .unwrap_or(500)
            })
            .with_body_from_request(move |request| {
                let body = request.body().map(Vec::as_slice).unwrap_or(&[]);
                post_body_state
                    .lock()
                    .map(|mut state| state.post_body(request.path(), body))
                    .unwrap_or_default()
            })
            .expect_at_least(0)
            .create();
        let put = server
            .mock("PUT", Matcher::Any)
            .with_status_code_from_request(move |request| {
                let body = request.body().map(Vec::as_slice).unwrap_or(&[]);
                put_status_state
                    .lock()
                    .map(|mut state| state.put_status(request.path(), body))
                    .unwrap_or(500)
            })
            .expect_at_least(0)
            .create();

        Self {
            server,
            state,
            _get: get,
            _post: post,
            _put: put,
        }
    }

    fn url(&self) -> String {
        self.server.url()
    }

    fn set_serial_state(&self, serial: u32, stub_state: StubState) -> TestResult<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("failed to lock stub state"))?;
        state.apply_state(serial, stub_state);
        Ok(())
    }

    fn assert_write_event(&self, serial: u32, secret: StubSecret, value: &str) -> TestResult<()> {
        let expected = format_write_event(serial, secret.name(), value);
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("failed to lock stub state"))?;
        assert!(
            state.write_events.iter().any(|event| event == &expected),
            "missing write event: {expected}\nobserved: {:?}",
            state.write_events
        );
        Ok(())
    }
}

fn encoded_object(secret_id: u8) -> Vec<u8> {
    format!("encoded-secret-{secret_id}").into_bytes()
}

fn format_write_event(serial: u32, secret_name: &str, value: &str) -> String {
    format!("DOTFILES_TEST_STUB_WRITE serial={serial} name={secret_name} value={value}")
}

fn parse_device_suffix(path: &str, suffix: &str) -> Option<u32> {
    let stripped = path.strip_prefix("/devices/")?;
    let (serial, tail) = stripped.split_once('/')?;
    if tail == suffix {
        serial.parse().ok()
    } else {
        None
    }
}

fn parse_object_path(path: &str) -> Option<(u32, u32)> {
    let stripped = path.strip_prefix("/devices/")?;
    let (serial, tail) = stripped.split_once("/objects/")?;
    Some((serial.parse().ok()?, tail.parse().ok()?))
}

fn parse_storage_suffix(path: &str, suffix: &str) -> Option<(u32, u8)> {
    let stripped = path.strip_prefix("/devices/")?;
    let (serial, tail) = stripped.split_once("/storage/")?;
    let (secret_id, actual_suffix) = tail.split_once('/')?;
    if actual_suffix == suffix {
        Some((serial.parse().ok()?, secret_id.parse().ok()?))
    } else {
        None
    }
}
