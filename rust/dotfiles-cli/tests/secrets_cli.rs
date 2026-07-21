#![cfg(feature = "secrets-internal-test-stub")]
//! `dotfiles secrets` の CLI 境界を feature-gated internal backend stub で検証する。
//!
//! Production command path は runtime env による real/stub 選択を持たない。この test target は
//! port ごとの初期条件 spec JSON を env で渡し、CLI 実行後に port ごとの最終状態観測 JSON だけを
//! 検証する。

use std::{
    fs,
    io::{ErrorKind, Read, Write},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use dotfiles_secrets::secrets_internal_test_stub_contract::{
    BWS_STUB_SPEC_ENV, GIT_STUB_SPEC_ENV, GPG_STUB_SPEC_ENV, STUB_OBSERVATION_PREFIX,
    YUBIKEY_STUB_SPEC_ENV,
};
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};
use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(15);
const PRIMARY_SERIAL: u32 = 2001;
const SPARE_SERIAL: u32 = 2002;
static PERSISTENT_STUB_SEQUENCE: AtomicU64 = AtomicU64::new(0);
// `crossterm` raw-mode changes are process-global on some PTY implementations.
// Management PIN tests deliberately exercise hidden terminal input, so serialize
// only PTY-backed child runs while ordinary pipe/recovery tests remain parallel.
static PTY_TEST_LOCK: Mutex<()> = Mutex::new(());

type TestResult<T> = anyhow::Result<T>;

struct CommandRun {
    success: bool,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl CommandRun {
    fn user_stdout(&self) -> String {
        strip_observation_lines(&self.stdout)
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
}

struct PtyRun {
    success: bool,
    exit_code: u32,
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
}

/// `required-features` を持つ専用 bin target の Cargo artifact を返す。
///
/// Cargo は integration test 実行前にこの target を
/// `secrets-internal-test-stub` 付きで build する。通常 binary の output path と
/// 異なるため、並行する featureless build が child を production backend へ差し替える
/// 余地はない。runtime flag による real/stub 選択は存在しない。
fn feature_stub_cli_binary() -> &'static str {
    env!("CARGO_BIN_EXE_dotfiles-secrets-internal-test-stub")
}

/// test-only YubiKey datastore の process 間保存先。
///
/// テストはこのファイルを読まず、initial fixture の投入と CLI stdout sentinel による最終状態観測だけを行う。
struct PersistentYubiKeyState {
    path: PathBuf,
}

impl PersistentYubiKeyState {
    fn new() -> Self {
        let unique = PERSISTENT_STUB_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        Self {
            path: std::env::temp_dir().join(format!(
                "dotfiles-yubikey-stub-{}-{nanos}-{unique}.json",
                std::process::id()
            )),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PersistentYubiKeyState {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

impl StubPorts {
    fn new(yubikey_spec: Value, bws_spec_value: Value) -> Self {
        Self {
            yubikey_spec,
            bws_spec_value,
            gpg_spec: empty_gpg_spec(),
            git_spec: empty_git_spec(),
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
            .env(GIT_STUB_SPEC_ENV, serde_json::to_string(&self.git_spec)?);
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
        Ok(())
    }
}

#[test]
fn setup_without_a_controlling_tty_fails_closed_before_device_access() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([fresh_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_without_controlling_tty_with_stub(
        ["yubikey", "setup", "--serial", "2001"],
        None,
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to open controlling terminal"));
    assert!(
        !run.stdout.contains(STUB_OBSERVATION_PREFIX),
        "TTY-less management command must not mutate the YubiKey stub: {}",
        run.stdout
    );
    Ok(())
}

#[test]
fn setup_rejects_a_tty_stdin_when_it_is_not_the_controlling_terminal() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([fresh_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run =
        run_pty_without_controlling_tty_with_stub(["yubikey", "setup", "--serial", "2001"], &stub)?;

    assert!(!run.success, "output: {}", run.output);
    assert!(run.output.contains("failed to open controlling terminal"));
    assert!(
        !run.output.contains(STUB_OBSERVATION_PREFIX),
        "TTY stdin without a controlling terminal must not mutate the YubiKey stub: {}",
        run.output
    );
    Ok(())
}

#[test]
fn setup_reads_hidden_piv_pin_from_pty() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([fresh_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pty_with_stub_interactive(
        ["yubikey", "setup", "--serial", "2001"],
        &[("YubiKey PIV PIN: ", "123456\n")],
        &stub,
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("YubiKey PIV PIN: "));
    Ok(())
}

#[test]
fn b0_bootstrap_reopens_and_reauthenticates_before_metadata_access() -> TestResult<()> {
    let device = json!({
        "serial": PRIMARY_SERIAL,
        "fixture": "fresh",
        "management_state": "b0-default",
        "key_metadata_requires_management_auth": true,
    });
    let stub = StubPorts::new(yubikey_spec([device]), bws_spec());
    let run = run_pty_with_stub_interactive(
        ["yubikey", "setup", "--serial", "2001"],
        &[("YubiKey PIV PIN: ", "123456\n")],
        &stub,
    )?;

    // The fixture rejects slot metadata unless the post-`set_protected`
    // handle authenticated again. A one-handle bootstrap regression therefore
    // cannot reach this successful setup observation.
    assert!(run.success, "output: {}", run.output);
    assert_eq!(
        run.final_yubikey()?["yubikeys"][PRIMARY_SERIAL.to_string()]["key_exists"],
        json!(true)
    );
    Ok(())
}

#[test]
fn management_pin_failures_do_not_fallback_or_mutate_storage() -> TestResult<()> {
    for management_state in [
        "wrong-pin",
        "pin-blocked",
        "protected-not-found-nondefault",
        "opaque-error",
        "partial",
    ] {
        let device = json!({
            "serial": PRIMARY_SERIAL,
            "fixture": "fresh",
            "management_state": management_state,
        });
        let state = PersistentYubiKeyState::new();
        let mut spec = yubikey_spec([device]);
        spec["persistence_path"] = json!(state.path());
        let stub = StubPorts::new(spec, bws_spec());
        let run = run_pty_with_stub_interactive(
            ["yubikey", "setup", "--serial", "2001"],
            &[("YubiKey PIV PIN: ", "123456\n")],
            &stub,
        )?;
        assert!(!run.success, "state {management_state}: {}", run.output);
        assert!(
            !run.output.contains("STUBSESSION") && !run.output.contains("default management key"),
            "state {management_state} must not use a fallback: {}",
            run.output
        );

        // A second process must observe the same terminal management failure.
        // This checks that a failed attempt did not persist an implicitly
        // usable management/storage state.
        let repeated = run_pty_with_stub_interactive(
            ["yubikey", "setup", "--serial", "2001"],
            &[("YubiKey PIV PIN: ", "123456\n")],
            &stub,
        )?;
        assert!(
            !repeated.success,
            "state {management_state} changed after failed management attempt: {}",
            repeated.output
        );
    }
    Ok(())
}

#[test]
fn put_without_a_controlling_tty_fails_closed_before_consuming_stdin_secret() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([writable_bitwarden_client_secret_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_without_controlling_tty_with_stub(
        [
            "yubikey",
            "put",
            "bitwarden-client-secret",
            "--serial",
            "2001",
            "--stdin",
        ],
        Some("new-token\r"),
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to open controlling terminal"));
    assert!(!run.stdout.contains(STUB_OBSERVATION_PREFIX));
    Ok(())
}

#[test]
fn put_reads_tty_prompt_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([writable_bitwarden_client_secret_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pty_with_stub_interactive(
        [
            "yubikey",
            "put",
            "bitwarden-client-secret",
            "--serial",
            "2001",
        ],
        &[
            ("YubiKey PIV PIN: ", "123456\n"),
            ("bitwarden-client-secret: ", "new-token\n"),
        ],
        &stub,
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("YubiKey PIV PIN: "));
    assert!(run.output.contains("bitwarden-client-secret: "));
    assert_stored_secret(
        &run.final_yubikey()?,
        PRIMARY_SERIAL,
        "bitwarden-client-secret",
        "new-token",
    );
    Ok(())
}

#[test]
fn status_lists_configured_secret_names_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["yubikey", "status", "--serial", "2001"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert_eq!(
        run.user_stdout(),
        "bw-email\nbw-password\nbitwarden-client-secret\n"
    );
    Ok(())
}

#[test]
fn status_lists_present_names_for_manifest_with_a_missing_secret_object() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([manifest_with_missing_secret_object_device_spec(
            PRIMARY_SERIAL,
        )]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["yubikey", "status", "--serial", "2001"], None, &stub)?;
    assert!(run.success, "stdout: {} stderr: {}", run.stdout, run.stderr);
    assert_eq!(
        run.user_stdout(),
        "bw-email\nbw-password\n"
    );
    Ok(())
}

#[test]
fn status_writes_configured_secret_names_to_tty_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pty_with_stub(["yubikey", "status", "--serial", "2001"], None, &stub)?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("bitwarden-client-secret"));
    assert!(!run.output.contains("YubiKey PIV PIN: "));
    Ok(())
}

#[test]
fn status_returns_the_reserved_storage_invalid_exit_code_for_observed_partial_storage()
-> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([manifestless_bitwarden_client_secret_device_spec(
            PRIMARY_SERIAL,
        )]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["yubikey", "status", "--serial", "2001"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert_eq!(
        run.exit_code,
        Some(i32::from(
            dotfiles_secrets::SECRET_STORAGE_STATUS_INVALID_EXIT_CODE
        ))
    );
    Ok(())
}

#[test]
fn put_returns_the_uninitialized_storage_exit_code_after_hidden_tty_pin() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([fresh_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pty_with_stub_interactive(
        [
            "yubikey",
            "put",
            "bitwarden-client-secret",
            "--serial",
            "2001",
            "--stdin",
        ],
        &[("YubiKey PIV PIN: ", "123456\n")],
        &stub,
    )?;

    assert_eq!(
        run.exit_code,
        u32::from(dotfiles_secrets::SECRET_STORAGE_UNINITIALIZED_EXIT_CODE),
        "output: {}",
        run.output
    );
    Ok(())
}

#[test]
fn status_returns_exit_42_only_for_manifest_or_reserved_object_inconsistency() -> TestResult<()> {
    for device in [corrupt_manifest_device_spec(PRIMARY_SERIAL)] {
        let stub = StubPorts::new(yubikey_spec([device]), bws_spec());
        let run = run_pipe_with_stub(["yubikey", "status", "--serial", "2001"], None, &stub)?;

        assert!(!run.success, "stdout: {}", run.stdout);
        assert_eq!(
            run.exit_code,
            Some(i32::from(
                dotfiles_secrets::SECRET_STORAGE_STATUS_INVALID_EXIT_CODE
            )),
            "stderr: {}",
            run.stderr
        );
    }
    Ok(())
}

#[test]
fn status_does_not_probe_slot_metadata_or_certificate_without_a_pin() -> TestResult<()> {
    for device in [
        manifest_without_reserved_key_device_spec(PRIMARY_SERIAL),
        manifestless_reserved_certificate_device_spec(PRIMARY_SERIAL),
    ] {
        let stub = StubPorts::new(yubikey_spec([device]), bws_spec());
        let run = run_pipe_with_stub(["yubikey", "status", "--serial", "2001"], None, &stub)?;
        assert!(run.success, "stderr: {}", run.stderr);
        assert!(!run.stderr.contains("YubiKey PIV PIN:"));
    }
    Ok(())
}

#[test]
fn clear_recovers_manifestless_and_corrupt_storage_across_cli_processes() -> TestResult<()> {
    for initial_device in [
        manifestless_bitwarden_client_secret_device_spec(PRIMARY_SERIAL),
        corrupt_manifest_device_spec(PRIMARY_SERIAL),
    ] {
        let state = PersistentYubiKeyState::new();
        let mut initial_spec = yubikey_spec([initial_device]);
        initial_spec["persistence_path"] = json!(state.path());
        let stub = StubPorts::new(initial_spec, bws_spec());

        let invalid = run_pipe_with_stub(["yubikey", "status", "--serial", "2001"], None, &stub)?;
        assert_eq!(
            invalid.exit_code,
            Some(i32::from(
                dotfiles_secrets::SECRET_STORAGE_STATUS_INVALID_EXIT_CODE
            )),
            "stderr: {}",
            invalid.stderr
        );

        // The YubiKey stub persists a zero-length `PUT DATA` value for every
        // cleared custom object, matching the fixed `yubikey` crate's
        // `save_object(id, &mut [])` API rather than deleting map entries.
        // Each invocation below is a separate CLI process sharing only that
        // persisted device state.
        let cleared = run_pty_with_stub_interactive(
            ["yubikey", "clear", "--serial", "2001", "--yes"],
            &[("YubiKey PIV PIN: ", "123456\n")],
            &stub,
        )?;
        assert!(cleared.success, "output: {}", cleared.output);
        assert_eq!(
            cleared.final_yubikey()?["yubikeys"][PRIMARY_SERIAL.to_string()]["stored_secrets"],
            json!({})
        );

        let empty = run_pipe_with_stub(["yubikey", "status", "--serial", "2001"], None, &stub)?;
        assert!(empty.success, "stderr: {}", empty.stderr);
        assert_eq!(empty.user_stdout(), "");

        let put = run_pty_with_stub_interactive(
            [
                "yubikey",
                "put",
                "bitwarden-client-secret",
                "--serial",
                "2001",
            ],
            &[
                ("YubiKey PIV PIN: ", "123456\n"),
                ("bitwarden-client-secret: ", "recovery-test-token\n"),
            ],
            &stub,
        )?;
        assert!(put.success, "output: {}", put.output);
        assert_stored_secret(
            &put.final_yubikey()?,
            PRIMARY_SERIAL,
            "bitwarden-client-secret",
            "recovery-test-token",
        );

        let recovered = run_pipe_with_stub(["yubikey", "status", "--serial", "2001"], None, &stub)?;
        assert!(recovered.success, "stderr: {}", recovered.stderr);
        assert_eq!(recovered.user_stdout(), "bitwarden-client-secret\n");
    }
    Ok(())
}

#[test]
fn clear_reads_hidden_piv_pin_after_explicit_confirmation() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([corrupt_manifest_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pty_with_stub_interactive(
        ["yubikey", "clear", "--serial", "2001", "--yes"],
        &[("YubiKey PIV PIN: ", "123456\n")],
        &stub,
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("YubiKey PIV PIN: "));
    Ok(())
}

#[test]
fn persistent_partial_storage_reports_present_names_without_clear() -> TestResult<()> {
    let state = PersistentYubiKeyState::new();
    let mut initial_spec = yubikey_spec([fresh_device_spec(PRIMARY_SERIAL)]);
    initial_spec["persistence_path"] = json!(state.path());
    let stub = StubPorts::new(initial_spec, bws_spec());

    let setup = run_pty_with_stub_interactive(
        ["yubikey", "setup", "--serial", "2001"],
        &[("YubiKey PIV PIN: ", "123456\n")],
        &stub,
    )?;
    assert!(setup.success, "output: {}", setup.output);

    let put = run_pty_with_stub_interactive(
        [
            "yubikey",
            "put",
            "bitwarden-client-secret",
            "--serial",
            "2001",
        ],
        &[
            ("YubiKey PIV PIN: ", "123456\n"),
            ("bitwarden-client-secret: ", "recovery-test-token\n"),
        ],
        &stub,
    )?;
    assert!(put.success, "output: {}", put.output);

    let status = run_pipe_with_stub(["yubikey", "status", "--serial", "2001"], None, &stub)?;
    assert!(status.success, "stderr: {}", status.stderr);
    assert_eq!(status.user_stdout(), "bitwarden-client-secret\n");
    Ok(())
}

#[test]
fn status_returns_exit_1_for_serial_resolution_or_device_io_failure() -> TestResult<()> {
    let serial_resolution_stub = StubPorts::new(yubikey_spec([]), bws_spec());
    let serial_resolution =
        run_pipe_with_stub(["yubikey", "status"], None, &serial_resolution_stub)?;
    assert!(
        !serial_resolution.success,
        "stdout: {}",
        serial_resolution.stdout
    );
    assert_eq!(serial_resolution.exit_code, Some(1));

    let device_io_stub = StubPorts::new(
        yubikey_spec([status_read_failure_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let device_io = run_pipe_with_stub(
        ["yubikey", "status", "--serial", "2001"],
        None,
        &device_io_stub,
    )?;
    assert!(!device_io.success, "stdout: {}", device_io.stdout);
    assert_eq!(device_io.exit_code, Some(1));
    Ok(())
}

#[test]
fn enroll_primary_without_a_controlling_tty_fails_closed_before_json_input() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([fresh_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_without_controlling_tty_with_stub(
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

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to open controlling terminal"));
    assert!(!run.stdout.contains(STUB_OBSERVATION_PREFIX));
    Ok(())
}

#[test]
fn enroll_primary_reads_tty_prompts_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([fresh_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pty_with_stub_interactive(
        ["yubikey", "enroll-primary", "--serial", "2001"],
        &[
            ("YubiKey PIV PIN: ", "123456\n"),
            ("bw-email: ", "u@example.com\n"),
            ("bw-password: ", "pw\n"),
            ("bitwarden-client-secret: ", "token\n"),
        ],
        &stub,
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("YubiKey PIV PIN: "));
    assert!(run.output.contains("bw-email: "));
    assert!(run.output.contains("bw-password: "));
    assert!(run.output.contains("bitwarden-client-secret: "));
    assert!(run.output.contains("\"role\": \"primary\""));
    let final_yubikey = run.final_yubikey()?;
    assert_stored_secret(&final_yubikey, PRIMARY_SERIAL, "bw-email", "u@example.com");
    assert_stored_secret(&final_yubikey, PRIMARY_SERIAL, "bw-password", "pw");
    assert_stored_secret(
        &final_yubikey,
        PRIMARY_SERIAL,
        "bitwarden-client-secret",
        "token",
    );
    Ok(())
}

#[test]
fn enroll_spare_without_a_controlling_tty_fails_closed_before_json_input() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([
            fresh_device_spec(PRIMARY_SERIAL),
            fresh_device_spec(SPARE_SERIAL),
        ]),
        bws_spec(),
    );
    let run = run_pipe_without_controlling_tty_with_stub(
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

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to open controlling terminal"));
    assert!(!run.stdout.contains(STUB_OBSERVATION_PREFIX));
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
    let run = run_pty_with_stub_interactive(
        [
            "yubikey",
            "enroll-spare",
            "--primary-serial",
            "2001",
            "--spare-serial",
            "2002",
        ],
        &[("YubiKey PIV PIN: ", "123456\n")],
        &stub,
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("\"role\": \"spare\""));
    let final_yubikey = run.final_yubikey()?;
    assert_stored_secret(&final_yubikey, SPARE_SERIAL, "bw-email", "u@example.com");
    assert_stored_secret(&final_yubikey, SPARE_SERIAL, "bw-password", "pw");
    assert_stored_secret(
        &final_yubikey,
        SPARE_SERIAL,
        "bitwarden-client-secret",
        "token",
    );
    Ok(())
}

#[test]
fn rotate_bws_token_without_a_controlling_tty_fails_closed_before_consuming_stdin_secret()
-> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_without_controlling_tty_with_stub(
        ["yubikey", "rotate-bws-token", "--serial", "2001", "--stdin"],
        Some("new-token\r"),
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to open controlling terminal"));
    assert!(!run.stdout.contains(STUB_OBSERVATION_PREFIX));
    Ok(())
}

#[test]
fn rotate_bws_token_reads_tty_prompt_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pty_with_stub_interactive(
        ["yubikey", "rotate-bws-token", "--serial", "2001"],
        &[
            ("YubiKey PIV PIN: ", "123456\n"),
            ("bitwarden-client-secret: ", "new-token\n"),
        ],
        &stub,
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("YubiKey PIV PIN: "));
    assert!(run.output.contains("bitwarden-client-secret: "));
    assert!(run.output.contains("\"serial\": 2001"));
    assert_stored_secret(
        &run.final_yubikey()?,
        PRIMARY_SERIAL,
        "bitwarden-client-secret",
        "new-token",
    );
    Ok(())
}

#[test]
fn rotate_bws_token_fails_closed_when_multiple_yubikeys_are_detected() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([
            provisioned_device_spec(PRIMARY_SERIAL),
            provisioned_device_spec(SPARE_SERIAL),
        ]),
        bws_spec(),
    );
    let run = run_pty_with_stub_interactive(
        ["yubikey", "rotate-bws-token"],
        &[("YubiKey PIV PIN: ", "123456\n")],
        &stub,
    )?;

    assert!(!run.success, "output: {}", run.output);
    assert!(
        run.output
            .contains("multiple YubiKeys detected; connect exactly one YubiKey and retry")
    );
    Ok(())
}

#[test]
fn rotate_bws_token_uses_explicit_serial_when_multiple_yubikeys_are_detected() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([
            provisioned_device_spec(PRIMARY_SERIAL),
            provisioned_device_spec(SPARE_SERIAL),
        ]),
        bws_spec(),
    );
    let run = run_pty_with_stub_interactive(
        ["yubikey", "rotate-bws-token", "--serial", "2002"],
        &[
            ("YubiKey PIV PIN: ", "123456\n"),
            ("bitwarden-client-secret: ", "new-token\n"),
        ],
        &stub,
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("YubiKey PIV PIN: "));
    assert!(run.output.contains("\"serial\": 2002"));
    assert!(!run.output.contains("rotate another YubiKey? [y/N]: "));
    let final_yubikey = run.final_yubikey()?;
    assert_stored_secret(
        &final_yubikey,
        SPARE_SERIAL,
        "bitwarden-client-secret",
        "new-token",
    );
    assert_stored_secret(
        &final_yubikey,
        PRIMARY_SERIAL,
        "bitwarden-client-secret",
        "token",
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
    let envelope = restore_envelope_json(PRIMARY_SERIAL);
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_backup_and_pass_remote(&envelope, RESTORE_PASS_REMOTE),
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
        json!(envelope)
    );
    assert_eq!(
        final_bws["resolved_secrets"]["password-store-remote"],
        json!(RESTORE_PASS_REMOTE)
    );
    Ok(())
}

#[test]
fn verify_yubikey_bws_check_reports_failed_for_invalid_backup_schema() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_backup_and_pass_remote("not-json", RESTORE_PASS_REMOTE),
    );
    let run = run_pipe_with_stub(
        ["verify-yubikey", "--serial", "2001", "--check", "bws"],
        None,
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"name\": \"bws\""));
    assert!(stdout.contains("\"status\": \"failed\""));
    Ok(())
}

#[test]
fn verify_yubikey_bws_check_reports_failed_for_invalid_primary_fingerprint() -> TestResult<()> {
    let envelope = restore_envelope_json(PRIMARY_SERIAL).replace(
        RESTORE_PRIMARY_FP,
        "0123456789ABCDEF0123456789abcdef01234567",
    );
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_backup_and_pass_remote(&envelope, RESTORE_PASS_REMOTE),
    );
    let run = run_pipe_with_stub(
        ["verify-yubikey", "--serial", "2001", "--check", "bws"],
        None,
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"name\": \"bws\""));
    assert!(stdout.contains("\"status\": \"failed\""));
    Ok(())
}

#[test]
fn verify_yubikey_bws_check_reports_failed_for_recipient_mismatch() -> TestResult<()> {
    let envelope = restore_envelope_json(SPARE_SERIAL);
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_backup_and_pass_remote(&envelope, RESTORE_PASS_REMOTE),
    );
    let run = run_pipe_with_stub(
        ["verify-yubikey", "--serial", "2001", "--check", "bws"],
        None,
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"name\": \"bws\""));
    assert!(stdout.contains("\"status\": \"failed\""));
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
            .contains("multiple YubiKeys detected; connect exactly one YubiKey and retry")
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
            .contains("multiple YubiKeys detected; connect exactly one YubiKey and retry"),
        "input precondition must fail before device resolution: {}",
        run.stderr
    );
    Ok(())
}

#[test]
fn verify_yubikey_all_runs_only_noninteractive_bws_recovery_check() -> TestResult<()> {
    let envelope = restore_envelope_json(PRIMARY_SERIAL);
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_backup_and_pass_remote(&envelope, RESTORE_PASS_REMOTE),
    );
    let run = run_pipe_with_stub(["verify-yubikey", "--serial", "2001", "--all"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    // `--all` は無対話の BWS recovery prerequisite だけを確認する。
    assert!(stdout.contains("\"name\": \"bws\""));
    assert!(!stdout.contains("\"status\": \"skipped\""));
    assert!(!stdout.contains("\"status\": \"failed\""));
    Ok(())
}

#[test]
fn verify_yubikey_no_args_leaves_only_bws_external_check_skipped() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["verify-yubikey", "--serial", "2001"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    // 引数なし実行では BWS 外部確認だけが machine-readable な skipped として残る。
    assert!(stdout.contains("\"name\": \"bws\""));
    assert!(stdout.contains("\"status\": \"skipped\""));
    Ok(())
}

#[test]
fn put_stdin_auto_detects_serial_and_fails_when_stdin_is_empty() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(
        ["yubikey", "put", "bitwarden-client-secret", "--stdin"],
        None,
        &stub,
    )?;

    assert!(!run.success, "should fail when stdin has no data");
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
fn put_updates_final_yubikey_spec_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([writable_bitwarden_client_secret_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let put_run = run_pty_with_stub_interactive(
        [
            "yubikey",
            "put",
            "bitwarden-client-secret",
            "--serial",
            "2001",
        ],
        &[
            ("YubiKey PIV PIN: ", "123456\n"),
            ("bitwarden-client-secret: ", "new-token\r"),
        ],
        &stub,
    )?;
    assert!(put_run.success, "output: {}", put_run.output);
    assert_stored_secret(
        &put_run.final_yubikey()?,
        PRIMARY_SERIAL,
        "bitwarden-client-secret",
        "new-token",
    );
    Ok(())
}

#[test]
fn status_does_not_output_seeded_secret_values_with_yubikey_path() -> TestResult<()> {
    let initial_device = seeded_device_spec(
        PRIMARY_SERIAL,
        "seed@example.com",
        "seed-pw",
        "seed-token",
    );
    let stub = StubPorts::new(yubikey_spec([initial_device]), bws_spec());
    let run = run_pipe_with_stub(["yubikey", "status", "--serial", "2001"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert_eq!(
        run.stdout,
        "bw-email\nbw-password\nbitwarden-client-secret\n"
    );
    assert!(!run.stdout.contains("seed-token"));
    Ok(())
}

#[test]
fn status_reports_present_name_without_decoding_storage_with_yubikey_path() -> TestResult<()> {
    let initial_device = storage_decode_error_device_spec(
        provisioned_device_spec(PRIMARY_SERIAL),
        "bitwarden-client-secret",
    );
    let stub = StubPorts::new(yubikey_spec([initial_device]), bws_spec());
    let run = run_pipe_with_stub(["yubikey", "status", "--serial", "2001"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.user_stdout().contains("bitwarden-client-secret"));
    Ok(())
}

#[test]
fn rotate_fails_when_seeded_storage_is_corrupt_with_yubikey_path() -> TestResult<()> {
    let initial_device =
        storage_decode_error_device_spec(provisioned_device_spec(PRIMARY_SERIAL), "bw-password");
    let stub = StubPorts::new(yubikey_spec([initial_device]), bws_spec());
    let run = run_pty_with_stub_interactive(
        ["yubikey", "rotate-bws-token", "--serial", "2001"],
        &[("YubiKey PIV PIN: ", "123456\n")],
        &stub,
    )?;

    assert!(!run.success, "output: {}", run.output);
    assert!(run.output.contains("failed to decode bw-password"));
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
    let mut command = Command::new(feature_stub_cli_binary());
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

    run_pipe_command(command, input)
}

/// Run a pipe-backed child after detaching it from the test process session.
///
/// `stdin = /dev/null` alone does not model a noninteractive invocation: a
/// child can still open the test runner's controlling `/dev/tty`. Creating a
/// new session before `exec` guarantees that opening `/dev/tty` fails, so the
/// management-PIN contract is tested before the JSON/pipe payload is consumed.
fn run_pipe_without_controlling_tty_with_stub<const N: usize>(
    args: [&str; N],
    input: Option<&str>,
    stub: &StubPorts,
) -> TestResult<CommandRun> {
    let mut command = Command::new(feature_stub_cli_binary());
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
    // SAFETY: the closure runs only in the forked child before exec and calls
    // the async-signal-safe `setsid(2)` wrapper without touching shared state.
    unsafe {
        command.pre_exec(|| rustix::process::setsid().map(|_| ()).map_err(Into::into));
    }
    stub.apply_to_command(&mut command)?;

    run_pipe_command(command, input)
}

fn run_pipe_without_stub<const N: usize>(
    args: [&str; N],
    input: Option<&str>,
) -> TestResult<CommandRun> {
    let mut command = Command::new(feature_stub_cli_binary());
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

    run_pipe_command(command, input)
}

/// Run a pipe-backed CLI child with a bounded lifetime.
///
/// `wait_with_output` has no timeout and leaves a child running if a prompt or
/// external operation blocks. Read both pipes concurrently, then always kill
/// and reap the child before returning an error. The timeout is diagnostic: it
/// turns a blocked child into a test failure while preserving the child output;
/// it is never interpreted as a command success or state transition.
fn run_pipe_command(mut command: Command, input: Option<&str>) -> TestResult<CommandRun> {
    let mut child = command.spawn()?;
    let mut stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            terminate_standard_child(&mut child);
            anyhow::bail!("failed to capture child stdout");
        }
    };
    let mut stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            terminate_standard_child(&mut child);
            anyhow::bail!("failed to capture child stderr");
        }
    };
    let stdout_handle = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let stderr_handle = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });

    let status = (|| -> TestResult<std::process::ExitStatus> {
        if let Some(input) = input {
            let mut stdin = child.stdin.take().context("failed to open child stdin")?;
            write_child_stdin(&mut stdin, input)?;
            // Some commands consume a line-delimited stdin payload and wait
            // for EOF before continuing. Close the pipe before polling the
            // child; keeping this writer through `wait` deadlocks the test.
            drop(stdin);
        }
        wait_standard_child(&mut child)
    })();
    if status.is_err() {
        terminate_standard_child(&mut child);
    }

    let stdout = stdout_handle
        .join()
        .map_err(|_| anyhow::anyhow!("failed to join pipe stdout reader"))??;
    let stderr = stderr_handle
        .join()
        .map_err(|_| anyhow::anyhow!("failed to join pipe stderr reader"))??;
    let status = match status {
        Ok(status) => status,
        Err(error) => {
            let stdout = String::from_utf8_lossy(&stdout);
            let stderr = String::from_utf8_lossy(&stderr);
            anyhow::bail!(
                "pipe child failed: {error}; captured stdout: {stdout:?}; captured stderr: {stderr:?}"
            );
        }
    };
    Ok(CommandRun {
        success: status.success(),
        exit_code: status.code(),
        stdout: String::from_utf8(stdout)?,
        stderr: String::from_utf8(stderr)?,
    })
}

fn run_pty_with_stub<const N: usize>(
    args: [&str; N],
    input: Option<&str>,
    stub: &StubPorts,
) -> TestResult<PtyRun> {
    let _pty_guard = PTY_TEST_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("PTY test lock is poisoned"))?;
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut command = CommandBuilder::new(feature_stub_cli_binary());
    command.arg("secrets");
    command.args(args);
    stub.apply_to_pty_command(&mut command)?;
    let mut child = pair.slave.spawn_command(command)?;
    drop(pair.slave);
    let mut reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(error) => {
            terminate_pty_child(&mut child);
            return Err(error.into());
        }
    };
    let output_handle = thread::spawn(move || {
        let mut output = String::new();
        reader.read_to_string(&mut output).map(|_| output)
    });

    let status = (|| -> TestResult<portable_pty::ExitStatus> {
        if let Some(input) = input {
            let mut writer = pair.master.take_writer()?;
            writer.write_all(input.as_bytes())?;
            drop(writer);
        }
        wait_pty_child(&mut child)
    })();
    if status.is_err() {
        terminate_pty_child(&mut child);
    }
    drop(pair.master);
    let output = output_handle
        .join()
        .map_err(|_| anyhow::anyhow!("failed to join PTY output reader"))??;
    let status = status?;
    Ok(PtyRun {
        success: status.success(),
        exit_code: status.exit_code(),
        output,
    })
}

/// stdin/stdout/stderr は PTY なので child 側では TTY に見えるが、PTY を controlling
/// terminal にしない。PIV PIN reader が stdin を fallback に使う回帰を検出する。
fn run_pty_without_controlling_tty_with_stub<const N: usize>(
    args: [&str; N],
    stub: &StubPorts,
) -> TestResult<PtyRun> {
    let _pty_guard = PTY_TEST_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("PTY test lock is poisoned"))?;
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut command = CommandBuilder::new(feature_stub_cli_binary());
    command.arg("secrets");
    command.args(args);
    command.set_controlling_tty(false);
    stub.apply_to_pty_command(&mut command)?;
    let mut child = pair.slave.spawn_command(command)?;
    drop(pair.slave);
    let mut reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(error) => {
            terminate_pty_child(&mut child);
            return Err(error.into());
        }
    };
    let output_handle = thread::spawn(move || {
        let mut output = String::new();
        reader.read_to_string(&mut output).map(|_| output)
    });
    let status = wait_pty_child(&mut child);
    if status.is_err() {
        terminate_pty_child(&mut child);
    }
    drop(pair.master);
    let output = output_handle
        .join()
        .map_err(|_| anyhow::anyhow!("failed to join PTY output reader"))??;
    let status = status?;
    Ok(PtyRun {
        success: status.success(),
        exit_code: status.exit_code(),
        output,
    })
}

/// prompt ごとに次の入力を送る PTY driver。
///
/// hidden input は一時的に raw mode を使うため、複数行を先行投入すると raw mode 復帰時に
/// 後続行が端末 queue から失われ得る。管理 PIN を持つ command は、実利用者と同じく prompt を
/// 観測してから次の値を送る。この helper は compile-time feature-stub binary を使う integration test 専用である。
fn run_pty_with_stub_interactive<const N: usize>(
    args: [&str; N],
    interactions: &[(&str, &str)],
    stub: &StubPorts,
) -> TestResult<PtyRun> {
    let _pty_guard = PTY_TEST_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("PTY test lock is poisoned"))?;
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut command = CommandBuilder::new(feature_stub_cli_binary());
    command.arg("secrets");
    command.args(args);
    stub.apply_to_pty_command(&mut command)?;
    let mut child = pair.slave.spawn_command(command)?;
    drop(pair.slave);

    let mut reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(error) => {
            terminate_pty_child(&mut child);
            return Err(error.into());
        }
    };
    let (chunks, chunk_receiver) = mpsc::channel();
    let output_handle = thread::spawn(move || {
        let mut output = String::new();
        let mut buffer = [0_u8; 1024];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            let chunk = String::from_utf8_lossy(&buffer[..read]).into_owned();
            output.push_str(&chunk);
            if chunks.send(chunk).is_err() {
                break;
            }
        }
        Ok::<_, std::io::Error>(output)
    });

    let status = (|| -> TestResult<portable_pty::ExitStatus> {
        let mut observed = String::new();
        let mut writer = pair.master.take_writer()?;
        for (prompt, input) in interactions {
            while !observed.contains(prompt) {
                let chunk = chunk_receiver
                    .recv_timeout(TIMEOUT)
                    .with_context(|| format!("timed out waiting for PTY prompt {prompt:?}"))?;
                observed.push_str(&chunk);
            }
            writer.write_all(input.as_bytes())?;
            writer.flush()?;
        }
        // The PTY writer must be closed before waiting so commands that read
        // until EOF (including the B0 management flow) can complete.
        drop(writer);
        wait_pty_child(&mut child)
    })();
    if status.is_err() {
        terminate_pty_child(&mut child);
    }
    drop(pair.master);
    let output = output_handle
        .join()
        .map_err(|_| anyhow::anyhow!("failed to join PTY output reader"))??;
    let status = status?;
    Ok(PtyRun {
        success: status.success(),
        exit_code: status.exit_code(),
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
            terminate_pty_child(child);
            anyhow::bail!("timed out waiting for PTY child process");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_standard_child(child: &mut std::process::Child) -> TestResult<std::process::ExitStatus> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            terminate_standard_child(child);
            anyhow::bail!("timed out waiting for pipe child process");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// Best-effort cleanup used on every helper error path. Both calls are made:
/// a failed kill must not skip reaping an already-exited child, and a failed
/// wait must not skip the kill attempt.
fn terminate_standard_child(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// See [`terminate_standard_child`]. PTY children need the same kill-and-reap
/// invariant when prompt delivery or output collection fails.
fn terminate_pty_child(child: &mut Box<dyn Child + Send + Sync>) {
    let _ = child.kill();
    let _ = child.wait();
}

fn yubikey_spec<const N: usize>(yubikeys: [Value; N]) -> Value {
    let yubikeys = Vec::from(yubikeys);
    json!({
        "yubikeys": yubikeys
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

fn manifest_with_missing_secret_object_device_spec(serial: u32) -> Value {
    json!({
        "serial": serial,
        "fixture": "manifest-with-missing-secret-object"
    })
}

fn writable_bitwarden_client_secret_device_spec(serial: u32) -> Value {
    json!({
        "serial": serial,
        "fixture": "writable-bitwarden-client-secret"
    })
}

fn manifestless_bitwarden_client_secret_device_spec(serial: u32) -> Value {
    json!({
        "serial": serial,
        "fixture": "manifestless-bitwarden-client-secret"
    })
}

fn corrupt_manifest_device_spec(serial: u32) -> Value {
    json!({ "serial": serial, "fixture": "corrupt-manifest" })
}

fn manifest_without_reserved_key_device_spec(serial: u32) -> Value {
    json!({ "serial": serial, "fixture": "manifest-without-reserved-key" })
}

fn manifestless_reserved_certificate_device_spec(serial: u32) -> Value {
    json!({ "serial": serial, "fixture": "manifestless-reserved-certificate" })
}

fn status_read_failure_device_spec(serial: u32) -> Value {
    json!({ "serial": serial, "fixture": "status-read-failure" })
}

fn seeded_device_spec(
    serial: u32,
    bw_email: &str,
    bw_password: &str,
    bitwarden_client_secret: &str,
) -> Value {
    json!({
        "serial": serial,
        "fixture": "seeded",
        "bw-email": bw_email,
        "bw-password": bw_password,
        "bitwarden-client-secret": bitwarden_client_secret
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

fn bws_spec_with_backup_and_pass_remote(envelope_json: &str, remote: &str) -> Value {
    json!({
        "fixture": "default-recovery-project",
        "gpg_secret_key_backup": envelope_json,
        "password_store_remote": remote
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
    // 既定 fixture は password-store-remote を 1 件持つ。pass-remote は BWS access token を YubiKey storage
    // の `bitwarden-client-secret` から読み、対話 PTY では上書き確認 [y] → `--url` 未指定なので可視プロンプト
    // （非秘匿の clone URL を通常入力でエコー）の順に入力して update する。最終観測で新値へ置換されたことを確認する。
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pty_with_stub(
        ["pass-remote", "register", "--serial", "2001"],
        Some(&format!("y\n{RESTORE_PASS_REMOTE}\n")),
        &stub,
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(
        !run.output
            .contains("bitwarden-client-secret (create/update): "),
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
    // 与えて既存 secret を update する。BWS access token は YubiKey storage から読み、非秘匿の URL は argv
    // から取得されるため、stdin 入力へは到達せず、最終 datastore が新値へ更新されることを観測する。
    let initial = bws_spec_with_pass_remote("git@github.com:owner/old-store.git");
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        initial,
    );
    let run = run_pipe_with_stub(
        [
            "pass-remote",
            "register",
            "--serial",
            "2001",
            "--url",
            RESTORE_PASS_REMOTE,
            "--yes",
        ],
        None,
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
    // BWS access token は YubiKey storage から読む。確認で停止するため、URL の入力（pipe/可視プロンプト）へは到達しない。
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["pass-remote", "register", "--serial", "2001"], None, &stub)?;

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
    // を与え、BWS access token は YubiKey storage から読み、pipe からは妥当な clone URL だけを渡して既存
    // secret を上書きする。pipe 入力経路（terminal でなければ stdin 1 行を読む分岐）と上書き挙動を駆動し、
    // 最終 BWS datastore が新値へ更新されたことを観測する。
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(
        ["pass-remote", "register", "--serial", "2001", "--yes"],
        Some(&format!("{RESTORE_PASS_REMOTE}\n")),
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
    // 既定 fixture は password-store-remote を 1 件持つ。対話 PTY で BWS access token を YubiKey storage から読み、
    // 上書き確認 [y] → 可視プロンプトへ domain 妥当でない clone URL を
    // 入力する。update 経路の URL 検証（application の PasswordStoreRemote::parse）で停止し、最終 datastore が
    // 元の値のまま不変であることを観測する。
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pty_with_stub(
        ["pass-remote", "register", "--serial", "2001"],
        Some("y\nnot-a-valid-clone-url\n"),
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
    let mut command = Command::new(feature_stub_cli_binary());
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
    let run = run_pipe_command(command, None)?;

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
  "bitwarden-client-secret": "token"
}
"#
}
