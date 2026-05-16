//! `dotfiles secrets` の CLI から利用者操作の結果をスタブ YubiKey で検証する。
//!
//! YubiKey PIV 操作は `secrets-test-stub` feature のメモリ上の端末に限定する。
//! stdin、stdout、stderr、TTY 判定、プロンプト入力は実プロセス境界を通し、保存系コマンドは
//! スタブ端末上で復号できる値まで確認する。

use std::{
    io::{ErrorKind, Read, Write},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};

#[path = "../src/secrets/test_stub_contract.rs"]
mod test_stub_contract;
use test_stub_contract::{PRIMARY_SERIAL, SPARE_SERIAL};

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

/// スタブ YubiKey に注入する保存済み値と、保存結果の検証で使う secret 名。
#[derive(Clone, Copy)]
enum StubSecret {
    BwEmail,
    BwPassword,
    BwsAccessToken,
}

impl StubSecret {
    /// clap env へ渡す `dotfiles secrets` の secret 名。
    fn name(self) -> &'static str {
        match self {
            Self::BwEmail => "bw-email",
            Self::BwPassword => "bw-password",
            Self::BwsAccessToken => "bws-access-token",
        }
    }

    /// 読み取り系テストで保存済み値を注入する環境変数。
    fn seed_env(self) -> &'static str {
        match self {
            Self::BwEmail => test_stub_contract::SEED_BW_EMAIL_ENV,
            Self::BwPassword => test_stub_contract::SEED_BW_PASSWORD_ENV,
            Self::BwsAccessToken => test_stub_contract::SEED_BWS_ACCESS_TOKEN_ENV,
        }
    }
}

/// 子プロセスの clap env に渡す device mock 状態を、テスト本文では型付きで表す。
#[derive(Clone, Copy)]
enum StubDeviceState {
    Fresh,
    Initialized,
    Provisioned,
    WritableBwsAccessToken,
}

impl StubDeviceState {
    fn value(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Initialized => "initialized",
            Self::Provisioned => "provisioned",
            Self::WritableBwsAccessToken => "writable-bws-access-token",
        }
    }
}

/// CLI 起動時にスタブ YubiKey へ渡す前提条件を列挙する。
#[derive(Clone, Copy)]
enum StubFixture {
    State(StubDeviceState),
    SerialState(u32, StubDeviceState),
    /// get / verify の読み取り対象として、端末へ保存済み secret を投入する。
    SeedSecret(StubSecret, &'static str),
    /// 指定 secret の保存 object を JSON ではないバイト列に置き換える。
    InvalidStoredObject(StubSecret),
    /// PIN 入力だけは固定値でなく、PTY 上の hidden prompt 経由で読む。
    ReadPinFromTty,
}

impl StubFixture {
    /// スタブ実装へ渡す fixture 指定を、子プロセス起動時の環境変数へ変換する。
    fn env(self) -> TestResult<(&'static str, String)> {
        match self {
            Self::State(state) => {
                Ok((test_stub_contract::STUB_STATE_ENV, state.value().to_owned()))
            }
            Self::SerialState(PRIMARY_SERIAL, state) => Ok((
                test_stub_contract::PRIMARY_STUB_STATE_ENV,
                state.value().to_owned(),
            )),
            Self::SerialState(SPARE_SERIAL, state) => Ok((
                test_stub_contract::SPARE_STUB_STATE_ENV,
                state.value().to_owned(),
            )),
            Self::SerialState(serial, _) => {
                anyhow::bail!("unsupported test stub serial state: {serial}")
            }
            Self::SeedSecret(secret, value) => Ok((secret.seed_env(), value.to_owned())),
            Self::InvalidStoredObject(secret) => Ok((
                test_stub_contract::CORRUPT_SECRET_ENV,
                secret.name().to_owned(),
            )),
            Self::ReadPinFromTty => {
                Ok((test_stub_contract::READ_PIN_FROM_TTY_ENV, "true".to_owned()))
            }
        }
    }
}

#[test]
fn setup_runs_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe(
        vec![
            "yubikey".to_owned(),
            "setup".to_owned(),
            "--serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
        ],
        None,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    Ok(())
}

#[test]
fn setup_rejects_initialized_stub_yubikey() -> TestResult<()> {
    let run = run_pipe_with_stub(
        vec![
            "yubikey".to_owned(),
            "setup".to_owned(),
            "--serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
        ],
        None,
        &[StubFixture::State(StubDeviceState::Initialized)],
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr
            .contains("YubiKey secret storage is already initialized")
    );
    Ok(())
}

#[test]
fn put_stores_non_tty_stdin_secret_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe_with_stub(
        [
            "yubikey".to_owned(),
            "put".to_owned(),
            "bws-access-token".to_owned(),
            "--serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
            "--stdin".to_owned(),
        ],
        Some("new-token\n"),
        &[StubFixture::State(StubDeviceState::WritableBwsAccessToken)],
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert_stub_stored_secret(
        &run.stderr,
        PRIMARY_SERIAL,
        StubSecret::BwsAccessToken,
        "new-token",
    );
    Ok(())
}

#[test]
fn put_rejects_empty_stdin_secret_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe_with_stub(
        [
            "yubikey".to_owned(),
            "put".to_owned(),
            "bws-access-token".to_owned(),
            "--serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
            "--stdin".to_owned(),
        ],
        Some("\n"),
        &[StubFixture::State(StubDeviceState::WritableBwsAccessToken)],
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("bws-access-token must not be empty"));
    Ok(())
}

#[test]
fn put_rejects_non_tty_without_serial_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe(
        ["yubikey", "put", "bws-access-token", "--stdin"],
        Some("secret\r"),
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("pass --serial in non-interactive use"));
    Ok(())
}

#[test]
fn put_stores_tty_prompt_secret_with_stub_yubikey() -> TestResult<()> {
    let run = run_pty_with_stub(
        vec![
            "yubikey".to_owned(),
            "put".to_owned(),
            "bws-access-token".to_owned(),
            "--serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
        ],
        Some("new-token\n"),
        &[StubFixture::State(StubDeviceState::WritableBwsAccessToken)],
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("bws-access-token: "));
    assert_stub_stored_secret(
        &run.output,
        PRIMARY_SERIAL,
        StubSecret::BwsAccessToken,
        "new-token",
    );
    Ok(())
}

#[test]
fn get_outputs_seeded_stub_secret_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe_with_stub(
        vec![
            "yubikey".to_owned(),
            "get".to_owned(),
            "bws-access-token".to_owned(),
            "--serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
        ],
        None,
        &[
            StubFixture::State(StubDeviceState::Provisioned),
            StubFixture::SeedSecret(StubSecret::BwEmail, "seed@example.com"),
            StubFixture::SeedSecret(StubSecret::BwPassword, "seed-pw"),
            StubFixture::SeedSecret(StubSecret::BwsAccessToken, "seed-token"),
        ],
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert_eq!(run.stdout, "seed-token");
    Ok(())
}

#[test]
fn get_fails_when_seeded_stub_storage_is_corrupt_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe_with_stub(
        vec![
            "yubikey".to_owned(),
            "get".to_owned(),
            "bws-access-token".to_owned(),
            "--serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
        ],
        None,
        &[
            StubFixture::State(StubDeviceState::Provisioned),
            StubFixture::InvalidStoredObject(StubSecret::BwsAccessToken),
        ],
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to decode bws-access-token"));
    Ok(())
}

#[test]
fn get_refuses_secret_output_to_tty_with_stub_yubikey() -> TestResult<()> {
    let run = run_pty_with_stub(
        vec![
            "yubikey".to_owned(),
            "get".to_owned(),
            "bws-access-token".to_owned(),
            "--serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
        ],
        None,
        &[StubFixture::State(StubDeviceState::Provisioned)],
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
fn enroll_primary_stores_non_tty_stdin_json_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe_with_stub(
        [
            "yubikey".to_owned(),
            "enroll-primary".to_owned(),
            "--serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
            "--stdin-json".to_owned(),
        ],
        Some(bootstrap_json()),
        &[],
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"role\": \"primary\""));
    assert!(run.stdout.contains("\"local_storage\": \"ok\""));
    assert_stub_stored_secret(
        &run.stderr,
        PRIMARY_SERIAL,
        StubSecret::BwEmail,
        "u@example.com",
    );
    assert_stub_stored_secret(&run.stderr, PRIMARY_SERIAL, StubSecret::BwPassword, "pw");
    assert_stub_stored_secret(
        &run.stderr,
        PRIMARY_SERIAL,
        StubSecret::BwsAccessToken,
        "token",
    );
    Ok(())
}

#[test]
fn enroll_primary_rejects_invalid_stdin_json_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe(
        [
            "yubikey".to_owned(),
            "enroll-primary".to_owned(),
            "--serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
            "--stdin-json".to_owned(),
        ],
        Some("{\"bw_email\":\"u@example.com\"}"),
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to parse bootstrap secret JSON"));
    Ok(())
}

#[test]
fn enroll_primary_stores_tty_prompt_secrets_with_stub_yubikey() -> TestResult<()> {
    let run = run_pty_with_stub(
        vec![
            "yubikey".to_owned(),
            "enroll-primary".to_owned(),
            "--serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
        ],
        Some("u@example.com\rpw\rnew-token\r"),
        &[],
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("bw-email: "));
    assert!(run.output.contains("bw-password: "));
    assert!(run.output.contains("bws-access-token: "));
    assert!(run.output.contains("\"role\": \"primary\""));
    assert_stub_stored_secret(
        &run.output,
        PRIMARY_SERIAL,
        StubSecret::BwEmail,
        "u@example.com",
    );
    assert_stub_stored_secret(&run.output, PRIMARY_SERIAL, StubSecret::BwPassword, "pw");
    assert_stub_stored_secret(
        &run.output,
        PRIMARY_SERIAL,
        StubSecret::BwsAccessToken,
        "new-token",
    );
    Ok(())
}

#[test]
fn enroll_spare_stores_non_tty_stdin_json_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe_with_stub(
        [
            "yubikey".to_owned(),
            "enroll-spare".to_owned(),
            "--spare-serial".to_owned(),
            SPARE_SERIAL.to_string(),
            "--stdin-json".to_owned(),
        ],
        Some(bootstrap_json()),
        &[],
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"role\": \"spare\""));
    assert!(run.stdout.contains(&serial_json_field(SPARE_SERIAL)));
    assert_stub_stored_secret(
        &run.stderr,
        SPARE_SERIAL,
        StubSecret::BwEmail,
        "u@example.com",
    );
    assert_stub_stored_secret(&run.stderr, SPARE_SERIAL, StubSecret::BwPassword, "pw");
    assert_stub_stored_secret(
        &run.stderr,
        SPARE_SERIAL,
        StubSecret::BwsAccessToken,
        "token",
    );
    Ok(())
}

#[test]
fn enroll_spare_rejects_same_primary_and_spare_serial_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe(
        [
            "yubikey".to_owned(),
            "enroll-spare".to_owned(),
            "--primary-serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
            "--spare-serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
        ],
        None,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr
            .contains("primary and spare YubiKey serial must be different")
    );
    Ok(())
}

#[test]
fn enroll_spare_rejects_non_tty_without_spare_serial_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe(
        [
            "yubikey".to_owned(),
            "enroll-spare".to_owned(),
            "--primary-serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
            "--stdin-json".to_owned(),
        ],
        Some(bootstrap_json()),
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr
            .contains("pass --spare-serial in non-interactive use")
    );
    Ok(())
}

#[test]
fn enroll_spare_rejects_non_tty_without_primary_serial_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe(
        vec![
            "yubikey".to_owned(),
            "enroll-spare".to_owned(),
            "--spare-serial".to_owned(),
            SPARE_SERIAL.to_string(),
        ],
        None,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr
            .contains("pass --primary-serial in non-interactive use")
    );
    Ok(())
}

#[test]
fn enroll_spare_uses_stub_yubikey_without_secret_reentry() -> TestResult<()> {
    let run = run_pipe_with_stub(
        [
            "yubikey".to_owned(),
            "enroll-spare".to_owned(),
            "--primary-serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
            "--spare-serial".to_owned(),
            SPARE_SERIAL.to_string(),
        ],
        None,
        &[
            StubFixture::SerialState(PRIMARY_SERIAL, StubDeviceState::Provisioned),
            StubFixture::SerialState(SPARE_SERIAL, StubDeviceState::Fresh),
        ],
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"role\": \"spare\""));
    Ok(())
}

#[test]
fn enroll_spare_reads_yubikey_pins_from_pty_with_stub_yubikey() -> TestResult<()> {
    let run = run_pty_with_stub(
        ["yubikey", "enroll-spare"],
        Some("123456\n123456\n"),
        &[
            StubFixture::SerialState(PRIMARY_SERIAL, StubDeviceState::Provisioned),
            StubFixture::SerialState(SPARE_SERIAL, StubDeviceState::Fresh),
            StubFixture::ReadPinFromTty,
        ],
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("YubiKey PIN: "));
    assert!(run.output.contains("\"role\": \"spare\""));
    Ok(())
}

#[test]
fn rotate_bws_token_stores_non_tty_stdin_secret_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe_with_stub(
        vec![
            "yubikey".to_owned(),
            "rotate-bws-token".to_owned(),
            "--serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
            "--stdin".to_owned(),
        ],
        Some("new-token\n"),
        &[StubFixture::State(StubDeviceState::Provisioned)],
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains(&serial_json_field(PRIMARY_SERIAL)));
    assert!(run.stdout.contains("\"local_storage\": \"ok\""));
    assert_stub_stored_secret(
        &run.stderr,
        PRIMARY_SERIAL,
        StubSecret::BwsAccessToken,
        "new-token",
    );
    Ok(())
}

#[test]
fn rotate_bws_token_fails_when_seeded_stub_storage_is_corrupt_with_stub_yubikey() -> TestResult<()>
{
    let run = run_pipe_with_stub(
        vec![
            "yubikey".to_owned(),
            "rotate-bws-token".to_owned(),
            "--serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
            "--stdin".to_owned(),
        ],
        Some("new-token\n"),
        &[
            StubFixture::State(StubDeviceState::Provisioned),
            StubFixture::InvalidStoredObject(StubSecret::BwPassword),
        ],
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to decode bw-password"));
    Ok(())
}

#[test]
fn rotate_bws_token_stores_tty_prompt_secret_with_stub_yubikey() -> TestResult<()> {
    let run = run_pty_with_stub(
        vec![
            "yubikey".to_owned(),
            "rotate-bws-token".to_owned(),
            "--serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
        ],
        Some("new-token\n"),
        &[StubFixture::State(StubDeviceState::Provisioned)],
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("bws-access-token: "));
    assert!(run.output.contains(&serial_json_field(PRIMARY_SERIAL)));
    assert_stub_stored_secret(
        &run.output,
        PRIMARY_SERIAL,
        StubSecret::BwsAccessToken,
        "new-token",
    );
    Ok(())
}

#[test]
fn rotate_bws_token_updates_spare_after_tty_device_replacement_with_stub_yubikey() -> TestResult<()>
{
    let run = run_pty_with_stub(
        ["yubikey", "rotate-bws-token"],
        Some("new-token\ny\n"),
        &[StubFixture::State(StubDeviceState::Provisioned)],
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains(&serial_json_field(PRIMARY_SERIAL)));
    assert!(run.output.contains(&serial_json_field(SPARE_SERIAL)));
    assert_stub_stored_secret(
        &run.output,
        PRIMARY_SERIAL,
        StubSecret::BwsAccessToken,
        "new-token",
    );
    assert_stub_stored_secret(
        &run.output,
        SPARE_SERIAL,
        StubSecret::BwsAccessToken,
        "new-token",
    );
    Ok(())
}

#[test]
fn verify_yubikey_fails_when_seeded_stub_storage_is_corrupt_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe_with_stub(
        vec![
            "verify-yubikey".to_owned(),
            "--serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
        ],
        None,
        &[
            StubFixture::State(StubDeviceState::Provisioned),
            StubFixture::InvalidStoredObject(StubSecret::BwEmail),
        ],
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to decode bw-email"));
    Ok(())
}

#[test]
fn verify_yubikey_checks_seeded_stub_storage_with_stub_yubikey() -> TestResult<()> {
    let run = run_pipe_with_stub(
        vec![
            "verify-yubikey".to_owned(),
            "--serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
        ],
        None,
        &[
            StubFixture::State(StubDeviceState::Provisioned),
            StubFixture::SeedSecret(StubSecret::BwEmail, "seed@example.com"),
            StubFixture::SeedSecret(StubSecret::BwPassword, "seed-pw"),
            StubFixture::SeedSecret(StubSecret::BwsAccessToken, "seed-token"),
        ],
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("\"local_storage\": \"ok\""));
    assert!(run.stdout.contains("\"bws\": \"skipped\""));
    Ok(())
}

#[test]
fn verify_yubikey_reads_yubikey_pin_from_pty_with_stub_yubikey() -> TestResult<()> {
    let run = run_pty_with_stub(
        vec![
            "verify-yubikey".to_owned(),
            "--serial".to_owned(),
            PRIMARY_SERIAL.to_string(),
        ],
        Some("123456\n"),
        &[
            StubFixture::State(StubDeviceState::Provisioned),
            StubFixture::ReadPinFromTty,
        ],
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("YubiKey PIN: "));
    assert!(run.output.contains("\"local_storage\": \"ok\""));
    Ok(())
}

/// 非 TTY 実行では stdin/stdout/stderr を明示的に pipe/null へ接続し、TTY 判定を実際に変える。
fn run_pipe<I, S>(args: I, input: Option<&str>) -> TestResult<CommandRun>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    run_pipe_with_stub(args, input, &[])
}

fn run_pipe_with_stub<I, S>(
    args: I,
    input: Option<&str>,
    fixtures: &[StubFixture],
) -> TestResult<CommandRun>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
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
    apply_stub_fixtures(&mut command, fixtures)?;

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

fn run_pty_with_stub<I, S>(
    args: I,
    input: Option<&str>,
    fixtures: &[StubFixture],
) -> TestResult<PtyRun>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
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
    apply_pty_stub_fixtures(&mut command, fixtures)?;
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

fn apply_stub_fixtures(command: &mut Command, fixtures: &[StubFixture]) -> TestResult<()> {
    for fixture in fixtures {
        let (key, value) = fixture.env()?;
        command.env(key, value);
    }
    Ok(())
}

/// 制御端末を使わない子プロセスへ、スタブ端末の初期状態と保存確認条件を渡す。
fn apply_pty_stub_fixtures(
    command: &mut CommandBuilder,
    fixtures: &[StubFixture],
) -> TestResult<()> {
    for fixture in fixtures {
        let (key, value) = fixture.env()?;
        command.env(key, value);
    }
    Ok(())
}

/// プロンプト待ちの失敗を検証停止にしないため、PTY 子プロセスは期限付きで待つ。
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

fn assert_stub_stored_secret(output: &str, serial: u32, secret: StubSecret, value: &str) {
    let expected = format!(
        "{} serial={} name={} value={}",
        test_stub_contract::WRITE_EVENT_PREFIX,
        serial,
        secret.name(),
        value
    );
    assert!(
        output.contains(&expected),
        "missing stored secret state: {expected}\n{output}"
    );
}

fn serial_json_field(serial: u32) -> String {
    format!("\"serial\": {serial}")
}

fn bootstrap_json() -> &'static str {
    r#"{
  "bw-email": "u@example.com",
  "bw-password": "pw",
  "bws-access-token": "token"
}
"#
}
