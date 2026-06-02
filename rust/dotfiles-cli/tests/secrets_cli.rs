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
    BW_LOGIN_STUB_SPEC_ENV, BWS_STUB_SPEC_ENV, GIT_STUB_SPEC_ENV, GPG_STUB_SPEC_ENV,
    STUB_OBSERVATION_PREFIX, YUBIKEY_STUB_SPEC_ENV,
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

    fn final_gpg(&self) -> TestResult<Value> {
        final_observation(&self.stdout, "gpg")
    }

    fn final_git(&self) -> TestResult<Value> {
        final_observation(&self.stdout, "git")
    }

    fn final_bw_login(&self) -> TestResult<Value> {
        final_observation(&self.stdout, "bw-login")
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

    fn final_bws(&self) -> TestResult<Value> {
        final_observation(&self.output, "bws")
    }
}

struct StubPorts {
    yubikey_spec: Value,
    bws_spec_value: Value,
    gpg_spec: Value,
    git_spec: Value,
    bw_login_spec: Value,
}

impl StubPorts {
    fn new(yubikey_spec: Value, bws_spec_value: Value) -> Self {
        Self {
            yubikey_spec,
            bws_spec_value,
            gpg_spec: empty_gpg_spec(),
            git_spec: empty_git_spec(),
            bw_login_spec: default_bw_login_spec(),
        }
    }

    fn with_gpg(mut self, gpg_spec: Value) -> Self {
        self.gpg_spec = gpg_spec;
        self
    }

    fn with_git(mut self, git_spec: Value) -> Self {
        self.git_spec = git_spec;
        self
    }

    fn with_bw_login(mut self, bw_login_spec: Value) -> Self {
        self.bw_login_spec = bw_login_spec;
        self
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
            )
            .env(GPG_STUB_SPEC_ENV, serde_json::to_string(&self.gpg_spec)?)
            .env(GIT_STUB_SPEC_ENV, serde_json::to_string(&self.git_spec)?)
            .env(
                BW_LOGIN_STUB_SPEC_ENV,
                serde_json::to_string(&self.bw_login_spec)?,
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
        command.env(GPG_STUB_SPEC_ENV, serde_json::to_string(&self.gpg_spec)?);
        command.env(GIT_STUB_SPEC_ENV, serde_json::to_string(&self.git_spec)?);
        command.env(
            BW_LOGIN_STUB_SPEC_ENV,
            serde_json::to_string(&self.bw_login_spec)?,
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
fn verify_yubikey_runs_bw_login_external_check_ok() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(
        ["verify-yubikey", "--serial", "2001", "--check", "bw-login"],
        None,
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"name\": \"bw-login\""));
    assert!(stdout.contains("\"status\": \"ok\""));
    Ok(())
}

#[test]
fn verify_yubikey_bw_login_external_check_fails_when_unreachable() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    )
    .with_bw_login(bw_login_spec_unreachable());
    let run = run_pipe_with_stub(
        ["verify-yubikey", "--serial", "2001", "--check", "bw-login"],
        None,
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"name\": \"bw-login\""));
    assert!(stdout.contains("\"status\": \"failed\""));
    Ok(())
}

#[test]
fn bw_login_runs_full_flow_with_yubikey_email() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    // OTP は非対話 pipe から 1 行読む。
    let run = run_pipe_with_stub(["bw-login", "--serial", "2001"], Some("123456\n"), &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"logged_in\": true"));
    assert!(stdout.contains("\"unlocked\": true"));
    let final_bw_login = run.final_bw_login()?;
    assert_eq!(final_bw_login["logged_in"], json!(true));
    assert_eq!(final_bw_login["unlocked"], json!(true));
    Ok(())
}

#[test]
fn bw_login_stops_when_login_fails() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    )
    .with_bw_login(bw_login_spec_with_login_failure());
    let run = run_pipe_with_stub(["bw-login", "--serial", "2001"], Some("123456\n"), &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
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

/// restore-gpg integration 用の primary fingerprint（lowercase hex 40）。
const RESTORE_PRIMARY_FP: &str = "0123456789abcdef0123456789abcdef01234567";
/// restore-gpg integration 用の authentication subkey keygrip（uppercase hex 40）。
const RESTORE_KEYGRIP: &str = "AABBCCDDEEFF00112233445566778899AABBCCDD";
/// restore-gpg integration 用の OpenSSH 公開鍵 1 行。
///
/// base64 本体は real adapter 同様に key blob として decode できる必要があるため、padding 込みで
/// 4 の倍数長の妥当な standard base64 を使う。
const RESTORE_SSH_LINE: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGdvb2RrZXlibG9iZ29vZGtleWJsb2Jnb29ka2V5MDE= restore@example";

/// serial を stub recipient fingerprint（lowercase hex 64）へ写像する（adapter stub と同じ規約）。
fn stub_recipient_fingerprint(serial: u32) -> String {
    let prefix = format!("{serial:08x}");
    let mut fingerprint = prefix.repeat(8);
    fingerprint.truncate(64);
    fingerprint
}

/// GPG stub が空でも spec 未設定にしない既定値（鍵なし）。
fn empty_gpg_spec() -> Value {
    json!({ "existing_keys": [], "keys": {} })
}

/// import 後の鍵を解決できる GPG stub spec を作る。
fn gpg_spec_with_importable_key() -> Value {
    json!({
        "existing_keys": [],
        "keys": {
            RESTORE_PRIMARY_FP: {
                "capabilities": ["encryption", "authentication", "signing"],
                "keygrip": RESTORE_KEYGRIP,
                "ssh_public_key": RESTORE_SSH_LINE
            }
        }
    })
}

/// restore-pass integration 用の `.gpg-id` recipient（Git stub 既定 recipient と整合する fingerprint）。
const RESTORE_PASS_RECIPIENT: &str = "0123456789ABCDEF0123456789ABCDEF01234567";

/// restore-pass の clone 後可読性確認が成功する GPG stub spec を作る。
///
/// restore-pass は ssh-agent を検査しない（gpg-agent SSH support の確認は restore-gpg の責務。設計 L116-124）
/// ため、agent identity 系の設定は持たない。`held_recipients` に `.gpg-id` recipient を含み、
/// `store_entry_decryptable` でサンプル entry の復号可否も成功させる。
fn gpg_spec_for_restore_pass() -> Value {
    json!({
        "existing_keys": [],
        "keys": {},
        "held_recipients": [RESTORE_PASS_RECIPIENT],
        "store_entry_decryptable": true
    })
}

/// 同一 primary fingerprint の鍵が既存する GPG stub spec を作る。
fn gpg_spec_with_existing_key() -> Value {
    json!({
        "existing_keys": [RESTORE_PRIMARY_FP],
        "keys": {
            RESTORE_PRIMARY_FP: {
                "capabilities": ["encryption", "authentication", "signing"],
                "keygrip": RESTORE_KEYGRIP,
                "ssh_public_key": RESTORE_SSH_LINE
            }
        }
    })
}

/// 接続中 serial の stub recipient に一致する encrypted envelope JSON を作る。
///
/// cipher stub は envelope body をそのまま復号済み backup として返すため、body は primary fingerprint
/// hex 文字列の base64（`MDEy...Nw==`）とし、keyring stub がそこから fingerprint を解決できるようにする。
fn restore_envelope_json(serial: u32) -> String {
    let pubkey = stub_recipient_fingerprint(serial);
    json!({
        "version": 1,
        "metadata": {
            "primary_fingerprint": RESTORE_PRIMARY_FP,
            "exported_at": "2026-05-31T00:00:00Z",
            "dek_alg": "aes-256-gcm",
            "recipient_kek_alg": "rsa-oaep-sha256"
        },
        "recipients": [
            {
                "yubikey_serial": serial.to_string(),
                "piv_slot": "82",
                "public_key_fingerprint": pubkey,
                "wrapped_dek": "d3JhcHBlZA=="
            }
        ],
        "ciphertext": {
            "nonce": "EBESExQVFhcYGRob",
            "body": "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWYwMTIzNDU2Nw==",
            "tag": "gIGCg4SFhoeIiYqLjI2Ojw=="
        }
    })
    .to_string()
}

/// gpg-secret-key-backup envelope を override した BWS spec を作る。
fn bws_spec_with_backup(envelope_json: &str) -> Value {
    json!({
        "fixture": "default-recovery-project",
        "gpg_secret_key_backup": envelope_json
    })
}

/// restore-pass integration 用の妥当な GitHub SSH clone URL。
const RESTORE_PASS_REMOTE: &str = "git@github.com:owner/password-store.git";

/// password-store-remote を妥当な clone URL へ override した BWS spec を作る。
fn bws_spec_with_pass_remote(remote: &str) -> Value {
    json!({
        "fixture": "default-recovery-project",
        "password_store_remote": remote
    })
}

/// Git stub が空でも spec 未設定にしない既定値（store なし・clone 後 `.gpg-id` あり）。
fn empty_git_spec() -> Value {
    json!({ "store_exists": false, "gpg_id_present": true })
}

/// login / unlock と reachability の双方が成立する bw-login stub spec を作る（既定）。
fn default_bw_login_spec() -> Value {
    json!({ "login_succeeds": true, "reachable": true })
}

/// login が失敗する bw-login stub spec を作る（停止条件の検証用）。
fn bw_login_spec_with_login_failure() -> Value {
    json!({ "login_succeeds": false, "reachable": true })
}

/// `bw` CLI へ到達できない bw-login stub spec を作る（`--check bw-login` 失敗の検証用）。
fn bw_login_spec_unreachable() -> Value {
    json!({ "login_succeeds": true, "reachable": false })
}

/// clone 前から `~/.password-store` が存在する Git stub spec を作る。
fn git_spec_with_existing_store() -> Value {
    json!({ "store_exists": true, "gpg_id_present": true })
}

/// clone 後 store に `.gpg-id` が無い（`pass` から読めない）Git stub spec を作る。
fn git_spec_with_unreadable_store() -> Value {
    json!({ "store_exists": false, "gpg_id_present": false })
}

#[test]
fn restore_pass_clones_store_and_confirms_readability_with_stub_paths() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_pass_remote(RESTORE_PASS_REMOTE),
    )
    .with_gpg(gpg_spec_for_restore_pass());
    let run = run_pipe_with_stub(["restore-pass", "--serial", "2001"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    assert!(
        stdout.contains("\"store_readable\": true"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains(".password-store"), "stdout: {stdout}");
    let final_git = run.final_git()?;
    assert_eq!(
        final_git["cloned_remotes"],
        json!([RESTORE_PASS_REMOTE]),
        "cloned remote must be observed"
    );
    Ok(())
}

#[test]
fn restore_pass_stops_when_store_already_exists_with_stub_paths() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_pass_remote(RESTORE_PASS_REMOTE),
    )
    .with_git(git_spec_with_existing_store());
    let run = run_pipe_with_stub(["restore-pass", "--serial", "2001"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr.contains("already exists"),
        "stderr: {}",
        run.stderr
    );
    let final_git = run.final_git()?;
    assert_eq!(
        final_git["cloned_remotes"],
        json!([]),
        "existing store must stop before clone"
    );
    Ok(())
}

#[test]
fn restore_pass_fails_when_remote_url_is_invalid_with_stub_paths() -> TestResult<()> {
    // 既定 fixture の password-store-remote は `https://example.invalid/repo.git` で domain 妥当でない。
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["restore-pass", "--serial", "2001"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr.contains("password-store-remote"),
        "stderr: {}",
        run.stderr
    );
    Ok(())
}

#[test]
fn restore_pass_fails_when_cloned_store_is_unreadable_with_stub_paths() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_pass_remote(RESTORE_PASS_REMOTE),
    )
    .with_gpg(gpg_spec_for_restore_pass())
    .with_git(git_spec_with_unreadable_store());
    let run = run_pipe_with_stub(["restore-pass", "--serial", "2001"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr.contains("pass cannot read the store"),
        "stderr: {}",
        run.stderr
    );
    // clone は成功するが可読性確認で失敗 → application は store を削除せず残す。再実行の安全性は既存 store
    // 停止条件に委ねる（spec L174 に自動削除は無い）ので、clone 観測は残ったまま。
    let final_git = run.final_git()?;
    assert_eq!(
        final_git["cloned_remotes"],
        json!([RESTORE_PASS_REMOTE]),
        "unreadable cloned store must be left in place (not rolled back)"
    );
    Ok(())
}

#[test]
fn restore_pass_errors_when_recipient_secret_key_is_absent_with_stub_paths() -> TestResult<()> {
    // entry が無い空 store で、`.gpg-id` recipient のいずれにも秘密鍵が無い（held_recipients を空にする）
    // → 可読性を確定できず error（空 store フォールバック）。store は削除せず残す。
    let mut gpg = gpg_spec_for_restore_pass();
    gpg["held_recipients"] = json!([]);
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_pass_remote(RESTORE_PASS_REMOTE),
    )
    .with_gpg(gpg)
    .with_git(
        json!({ "store_exists": false, "gpg_id_present": true, "sample_entry_present": false }),
    );
    let run = run_pipe_with_stub(["restore-pass", "--serial", "2001"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr.contains("secret keys are not in the keyring"),
        "stderr: {}",
        run.stderr
    );
    let final_git = run.final_git()?;
    assert_eq!(
        final_git["cloned_remotes"],
        json!([RESTORE_PASS_REMOTE]),
        "empty store with no held recipient secret key must be left in place (not rolled back)"
    );
    Ok(())
}

#[test]
fn pass_remote_register_overwrites_existing_secret_with_tty_confirmation() -> TestResult<()> {
    // 既定 fixture は password-store-remote を 1 件持つ。pass-remote は YubiKey を使わず、BWS 書込み用の
    // provisioning 用 access token を hidden prompt から受け取る。対話 PTY で provisioning token（stub の
    // datastore token と一致する `token`）→ 上書き確認 [y] → `--url` 未指定なので可視プロンプト（非秘匿の
    // clone URL を通常入力でエコー）の順に入力して update する。最終観測で新値へ置換されたことを確認する。
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pty_with_stub(
        ["pass-remote", "register"],
        Some(&format!("token\ny\n{RESTORE_PASS_REMOTE}\n")),
        &stub,
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(
        run.output.contains("provisioning-access-token: "),
        "output: {}",
        run.output
    );
    assert!(
        run.output.contains("password-store-remote: "),
        "output: {}",
        run.output
    );
    let final_bws = run.final_bws()?;
    assert_eq!(
        final_bws["resolved_secrets"]["password-store-remote"],
        json!(RESTORE_PASS_REMOTE),
        "overwritten password-store-remote must be observed"
    );
    Ok(())
}

#[test]
fn pass_remote_register_overwrites_existing_secret_from_url_argument_with_yes() -> TestResult<()> {
    // 既定 fixture は password-store-remote を 1 件持つ。非対話実行（非 TTY）で `--url` 引数と `--yes` を
    // 与えて既存 secret を update する。BWS 書込み用 provisioning 用 access token は pipe（stdin）の 1 行目で
    // 渡す（stub datastore token と一致する `token`）。非秘匿の URL は argv から取得され、可視プロンプト/pipe
    // の URL 入力へは到達せず、最終 datastore が新値へ更新されることを観測する。
    let initial = bws_spec_with_pass_remote("git@github.com:owner/old-store.git");
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        initial,
    );
    let run = run_pipe_with_stub(
        [
            "pass-remote",
            "register",
            "--url",
            RESTORE_PASS_REMOTE,
            "--yes",
        ],
        Some("token\n"),
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    let final_bws = run.final_bws()?;
    assert_eq!(
        final_bws["resolved_secrets"]["password-store-remote"],
        json!(RESTORE_PASS_REMOTE),
        "clone URL supplied via --url must overwrite the existing value with --yes"
    );
    Ok(())
}

#[test]
fn pass_remote_register_stops_non_interactive_overwrite_without_yes() -> TestResult<()> {
    // 非対話実行（pipe stdin）で既存 secret を上書きしようとし、`--yes` 未指定なら確認段階で停止する。
    // BWS 書込み用 provisioning 用 access token は pipe の 1 行目で渡す。確認で停止するため、URL の入力
    // （pipe/可視プロンプト）へは到達しない。
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["pass-remote", "register"], Some("token\n"), &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr.contains("was not confirmed"),
        "stderr: {}",
        run.stderr
    );
    let final_bws = run.final_bws()?;
    assert_eq!(
        final_bws["resolved_secrets"]["password-store-remote"],
        json!("https://example.invalid/repo.git"),
        "declined overwrite must leave the existing value unchanged"
    );
    Ok(())
}

#[test]
fn pass_remote_register_overwrites_existing_secret_via_stdin_pipe_with_yes() -> TestResult<()> {
    // 既定 fixture は password-store-remote を 1 件持つ。非対話実行（stdin pipe・非 TTY）で `--yes`
    // を与え、pipe の 1 行目で BWS 書込み用 provisioning 用 access token（stub datastore token と一致する
    // `token`）を、2 行目で妥当な clone URL を渡して既存 secret を上書きする。pipe 入力経路（terminal で
    // なければ stdin 1 行を読む分岐）と上書き挙動を駆動し、最終 BWS datastore が新値へ更新された
    // ことを観測する。
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(
        ["pass-remote", "register", "--yes"],
        Some(&format!("token\n{RESTORE_PASS_REMOTE}\n")),
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    let final_bws = run.final_bws()?;
    assert_eq!(
        final_bws["resolved_secrets"]["password-store-remote"],
        json!(RESTORE_PASS_REMOTE),
        "pipe-supplied clone URL must overwrite the existing value with --yes"
    );
    Ok(())
}

#[test]
fn pass_remote_register_stops_when_input_url_is_invalid() -> TestResult<()> {
    // 既定 fixture は password-store-remote を 1 件持つ。対話 PTY で provisioning 用 access token（stub
    // datastore token と一致する `token`）→ 上書き確認 [y] → 可視プロンプトへ domain 妥当でない clone URL を
    // 入力する。update 経路の URL 検証（application の PasswordStoreRemote::parse）で停止し、最終 datastore が
    // 元の値のまま不変であることを観測する。
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pty_with_stub(
        ["pass-remote", "register"],
        Some("token\ny\nnot-a-valid-clone-url\n"),
        &stub,
    )?;

    assert!(!run.success, "output: {}", run.output);
    let final_bws = run.final_bws()?;
    assert_eq!(
        final_bws["resolved_secrets"]["password-store-remote"],
        json!("https://example.invalid/repo.git"),
        "invalid clone URL must stop the update and leave the existing value unchanged"
    );
    Ok(())
}

#[test]
fn restore_gpg_imports_key_and_registers_ssh_with_stub_paths() -> TestResult<()> {
    let envelope = restore_envelope_json(PRIMARY_SERIAL);
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_backup(&envelope),
    )
    .with_gpg(gpg_spec_with_importable_key());
    let run = run_pipe_with_stub(["restore-gpg", "--serial", "2001"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    assert!(stdout.contains(&format!(
        "\"primary_fingerprint\": \"{RESTORE_PRIMARY_FP}\""
    )));
    assert!(stdout.contains("\"ssh_support_ready\": true"));
    let final_gpg = run.final_gpg()?;
    assert_eq!(
        final_gpg["imported_keys"],
        json!([RESTORE_PRIMARY_FP]),
        "imported key must be observed"
    );
    assert_eq!(
        final_gpg["registered_keygrips"],
        json!([RESTORE_KEYGRIP]),
        "authentication subkey keygrip must be registered"
    );
    Ok(())
}

#[test]
fn restore_gpg_stops_when_existing_key_collides_with_stub_paths() -> TestResult<()> {
    let envelope = restore_envelope_json(PRIMARY_SERIAL);
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_backup(&envelope),
    )
    .with_gpg(gpg_spec_with_existing_key());
    let run = run_pipe_with_stub(["restore-gpg", "--serial", "2001"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr.contains("already exists"),
        "stderr: {}",
        run.stderr
    );
    Ok(())
}

#[test]
fn export_ssh_public_key_writes_openssh_line_with_stub_paths() -> TestResult<()> {
    let stub =
        StubPorts::new(yubikey_spec([]), bws_spec()).with_gpg(gpg_spec_with_importable_key());
    // `dotfiles gpg export-ssh-public-key` は top-level command なので secrets 経由ではない。
    let mut command = Command::new(env!("CARGO_BIN_EXE_dotfiles"));
    command
        .arg("gpg")
        .args([
            "export-ssh-public-key",
            "--primary-fingerprint",
            RESTORE_PRIMARY_FP,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    stub.apply_to_command(&mut command)?;
    let output = command.spawn()?.wait_with_output()?;
    let run = CommandRun {
        success: output.status.success(),
        stdout: String::from_utf8(output.stdout)?,
        stderr: String::from_utf8(output.stderr)?,
    };

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(
        run.user_stdout().contains(RESTORE_SSH_LINE),
        "stdout: {}",
        run.stdout
    );
    Ok(())
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
