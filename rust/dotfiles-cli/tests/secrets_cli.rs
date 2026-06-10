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

#[derive(Debug)]
struct PtyOutputReaderJoinFailed;

impl std::fmt::Display for PtyOutputReaderJoinFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to join PTY output reader")
    }
}

impl std::error::Error for PtyOutputReaderJoinFailed {}

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

/// `yubikey setup` は単一接続 YubiKey を自動選択して初期化経路を完了する。
#[test]
fn setup_runs_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([fresh_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["yubikey", "setup"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    Ok(())
}

/// `yubikey put --stdin` は非 TTY stdin の 1 secret を対象 YubiKey へ保存する。
#[test]
fn put_reads_non_tty_stdin_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([writable_bws_access_token_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(
        ["yubikey", "put", "bws-access-token", "--stdin"],
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

/// `yubikey put bws-access-token` は provisioning 用 token と同一値を fail-closed で拒否する。
#[test]
fn put_rejects_same_token_as_provisioning_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([writable_bws_access_token_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(
        ["yubikey", "put", "bws-access-token", "--stdin"],
        Some("token\n"),
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr.contains(
            "refusing to store bws-access-token: recovery token must differ from the provisioning token"
        ),
        "stderr: {}",
        run.stderr
    );
    let final_yubikey = run.final_yubikey()?;
    assert_eq!(
        final_yubikey["yubikeys"][PRIMARY_SERIAL.to_string()]["stored_secrets"]
            .get("bws-access-token"),
        None,
        "rejected put must not persist the provisioning token into YubiKey storage"
    );
    Ok(())
}

/// `yubikey put bws-access-token` は provenance marker 欠落を fail-closed で拒否する。
#[test]
fn put_rejects_bws_access_token_when_provenance_marker_is_missing() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([writable_bws_access_token_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_pass_remote_note(RESTORE_PASS_REMOTE, Some("")),
    );
    let run = run_pipe_with_stub(
        ["yubikey", "put", "bws-access-token", "--stdin"],
        Some("new-token\n"),
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr.contains(
            "refusing to store bws-access-token: password-store-remote is missing provisioning token provenance"
        ),
        "stderr: {}",
        run.stderr
    );
    assert_eq!(
        run.final_yubikey()?["yubikeys"][PRIMARY_SERIAL.to_string()]["stored_secrets"]
            .get("bws-access-token"),
        None
    );
    Ok(())
}

/// `yubikey put bws-access-token` は provenance marker 改ざんを fail-closed で拒否する。
#[test]
fn put_rejects_bws_access_token_when_provenance_marker_is_tampered() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([writable_bws_access_token_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_pass_remote_note(
            RESTORE_PASS_REMOTE,
            Some("dotfiles-provisioning-bws-access-token-id=123E4567-E89B-12D3-A456-426614174000"),
        ),
    );
    let run = run_pipe_with_stub(
        ["yubikey", "put", "bws-access-token", "--stdin"],
        Some("new-token\n"),
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr.contains(
            "refusing to store bws-access-token: password-store-remote is missing provisioning token provenance"
        ),
        "stderr: {}",
        run.stderr
    );
    assert_eq!(
        run.final_yubikey()?["yubikeys"][PRIMARY_SERIAL.to_string()]["stored_secrets"]
            .get("bws-access-token"),
        None
    );
    Ok(())
}

/// `yubikey put` は TTY では hidden prompt 経由で 1 secret を保存する。
#[test]
fn put_reads_tty_prompt_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([writable_bws_access_token_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pty_with_stub(
        ["yubikey", "put", "bws-access-token"],
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

/// `yubikey get` は pipe stdout に限り secret 本文だけを書き出す。
#[test]
fn get_writes_secret_to_pipe_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["yubikey", "get", "bws-access-token"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert_eq!(run.user_stdout(), "token");
    Ok(())
}

/// `yubikey get` は TTY stdout へ secret 本文を出さず停止する。
#[test]
fn get_refuses_secret_output_to_tty_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pty_with_stub(["yubikey", "get", "bws-access-token"], None, &stub)?;

    assert!(!run.success, "output: {}", run.output);
    assert!(run.output.contains("refusing to write secret to terminal"));
    Ok(())
}

/// primary enrollment は単一接続 YubiKey に対して TTY prompt 入力だけで secret を登録する。
#[test]
fn enroll_primary_reads_tty_prompts_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([fresh_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pty_with_stub(
        ["yubikey", "enroll-primary"],
        Some("u@example.com\npw\nrecovery-token\n"),
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
    assert_stored_secret(
        &final_yubikey,
        PRIMARY_SERIAL,
        "bws-access-token",
        "recovery-token",
    );
    Ok(())
}

/// 削除済みの `--stdin-json` を primary enrollment で受け付けない CLI 境界を固定する。
#[test]
fn enroll_primary_rejects_removed_stdin_json_option() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([fresh_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(
        ["yubikey", "enroll-primary", "--stdin-json"],
        Some(bootstrap_json()),
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("unexpected argument '--stdin-json'"));
    Ok(())
}

/// 削除済みの `--serial` を primary enrollment で受け付けない CLI 境界を固定する。
#[test]
fn enroll_primary_rejects_removed_serial_option() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([fresh_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(
        ["yubikey", "enroll-primary", "--serial", "2001"],
        None,
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("unexpected argument '--serial'"));
    Ok(())
}

/// spare enrollment は単一接続 YubiKey に対して TTY prompt 入力だけで secret を登録する。
#[test]
fn enroll_spare_reads_tty_prompts_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(yubikey_spec([fresh_device_spec(SPARE_SERIAL)]), bws_spec());
    let run = run_pty_with_stub(
        ["yubikey", "enroll-spare"],
        Some("u@example.com\npw\nrecovery-token\n"),
        &stub,
    )?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("bw-email: "));
    assert!(run.output.contains("bw-password: "));
    assert!(run.output.contains("bws-access-token: "));
    assert!(run.output.contains("\"role\": \"spare\""));
    assert!(!run.output.contains("\"serial\""));
    let final_yubikey = run.final_yubikey()?;
    assert_stored_secret(&final_yubikey, SPARE_SERIAL, "bw-email", "u@example.com");
    assert_stored_secret(&final_yubikey, SPARE_SERIAL, "bw-password", "pw");
    assert_stored_secret(
        &final_yubikey,
        SPARE_SERIAL,
        "bws-access-token",
        "recovery-token",
    );
    Ok(())
}

/// spare enrollment は複数 YubiKey 接続時に serial 選択へ進まず接続数の操作制約で停止する。
#[test]
fn enroll_spare_stops_when_multiple_yubikeys_are_connected() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([
            fresh_device_spec(PRIMARY_SERIAL),
            fresh_device_spec(SPARE_SERIAL),
        ]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["yubikey", "enroll-spare"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr
            .contains("multiple YubiKeys detected; connect exactly one YubiKey and retry"),
        "stderr: {}",
        run.stderr
    );
    Ok(())
}

/// 削除済みの `--stdin-json` を spare enrollment で受け付けない CLI 境界を固定する。
#[test]
fn enroll_spare_rejects_removed_stdin_json_option() -> TestResult<()> {
    let stub = StubPorts::new(yubikey_spec([fresh_device_spec(SPARE_SERIAL)]), bws_spec());
    let run = run_pipe_with_stub(
        ["yubikey", "enroll-spare", "--stdin-json"],
        Some(bootstrap_json()),
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("unexpected argument '--stdin-json'"));
    Ok(())
}

/// `rotate-bws-token --stdin` は非 TTY stdin の token を既存 YubiKey storage へ保存し直す。
#[test]
fn rotate_bws_token_reads_non_tty_stdin_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(
        ["yubikey", "rotate-bws-token", "--stdin"],
        Some("new-token\r"),
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"name\": \"local-storage\""));
    assert!(stdout.contains("\"status\": \"ok\""));
    assert!(!stdout.contains("\"name\": \"bws\""));
    assert!(!stdout.contains("\"name\": \"bw-login\""));
    assert!(!stdout.contains("\"serial\""));
    assert_stored_secret(
        &run.final_yubikey()?,
        PRIMARY_SERIAL,
        "bws-access-token",
        "new-token\r",
    );
    Ok(())
}

/// `rotate-bws-token` は TTY prompt から token を読み、要約に serial を露出しない。
#[test]
fn rotate_bws_token_reads_tty_prompt_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pty_with_stub(["yubikey", "rotate-bws-token"], Some("new-token\n"), &stub)?;

    assert!(run.success, "output: {}", run.output);
    assert!(run.output.contains("bws-access-token: "));
    assert!(run.output.contains("\"name\": \"local-storage\""));
    assert!(!run.output.contains("\"name\": \"bws\""));
    assert!(!run.output.contains("\"name\": \"bw-login\""));
    assert!(!run.output.contains("\"serial\""));
    assert_stored_secret(
        &run.final_yubikey()?,
        PRIMARY_SERIAL,
        "bws-access-token",
        "new-token",
    );
    Ok(())
}

/// BWS token rotate は複数 YubiKey 接続時に secret 入力後でも device 選択へ進まず停止する。
#[test]
fn rotate_bws_token_stops_when_multiple_yubikeys_are_connected() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([
            provisioned_device_spec(PRIMARY_SERIAL),
            provisioned_device_spec(SPARE_SERIAL),
        ]),
        bws_spec(),
    );
    let run = run_pty_with_stub(["yubikey", "rotate-bws-token"], Some("new-token\n"), &stub)?;

    assert!(!run.success, "output: {}", run.output);
    assert!(run.output.contains("connect exactly one YubiKey and retry"));
    assert!(!run.output.contains("\"serial\""));
    Ok(())
}

/// `verify-yubikey` は local storage 確認を実行し、未要求の BWS 観測を発生させない。
#[test]
fn verify_yubikey_runs_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["verify-yubikey"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"name\": \"local-storage\""));
    assert!(stdout.contains("\"status\": \"ok\""));
    assert!(stdout.contains("\"name\": \"bws\""));
    assert!(stdout.contains("\"status\": \"skipped\""));
    assert!(!run.has_bws_observation());
    Ok(())
}

/// `verify-yubikey --check bws` は BWS secret 取得と envelope recipient 照合を確認する。
#[test]
fn verify_yubikey_runs_bws_external_check() -> TestResult<()> {
    let envelope = restore_envelope_json(PRIMARY_SERIAL);
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_backup_and_pass_remote(&envelope, RESTORE_PASS_REMOTE),
    );
    let run = run_pipe_with_stub(["verify-yubikey", "--check", "bws"], None, &stub)?;

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

/// 削除済みの `--serial` を `verify-yubikey` で受け付けない CLI 境界を固定する。
#[test]
fn verify_yubikey_rejects_removed_serial_option() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["verify-yubikey", "--serial", "2001"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("unexpected argument '--serial'"));
    Ok(())
}

/// BWS check は schema として壊れた backup envelope を failed として報告する。
#[test]
fn verify_yubikey_bws_check_reports_failed_for_invalid_backup_schema() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_backup_and_pass_remote("not-json", RESTORE_PASS_REMOTE),
    );
    let run = run_pipe_with_stub(["verify-yubikey", "--check", "bws"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"name\": \"bws\""));
    assert!(stdout.contains("\"status\": \"failed\""));
    Ok(())
}

/// BWS check は primary fingerprint が canonical でない backup envelope を failed として報告する。
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
    let run = run_pipe_with_stub(["verify-yubikey", "--check", "bws"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"name\": \"bws\""));
    assert!(stdout.contains("\"status\": \"failed\""));
    Ok(())
}

/// BWS check は接続中 YubiKey と一致しない recipient だけの backup envelope を failed として報告する。
#[test]
fn verify_yubikey_bws_check_reports_failed_for_recipient_mismatch() -> TestResult<()> {
    let envelope = restore_envelope_json(SPARE_SERIAL);
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_backup_and_pass_remote(&envelope, RESTORE_PASS_REMOTE),
    );
    let run = run_pipe_with_stub(["verify-yubikey", "--check", "bws"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"name\": \"bws\""));
    assert!(stdout.contains("\"status\": \"failed\""));
    Ok(())
}

/// BWS check は接続中 YubiKey に一致していても 1 recipient だけの backup envelope を failed として報告する。
#[test]
fn verify_yubikey_bws_check_reports_failed_for_single_recipient_backup() -> TestResult<()> {
    let envelope = restore_single_recipient_envelope_json(PRIMARY_SERIAL);
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_backup_and_pass_remote(&envelope, RESTORE_PASS_REMOTE),
    );
    let run = run_pipe_with_stub(["verify-yubikey", "--check", "bws"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"name\": \"bws\""));
    assert!(stdout.contains("\"status\": \"failed\""));
    Ok(())
}

/// `verify-yubikey` は複数接続時に serial 指定を要求せず、接続数を 1 件にする停止条件を返す。
#[test]
fn verify_yubikey_requires_exactly_one_connected_device() -> TestResult<()> {
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

/// `verify-yubikey` は単一接続 YubiKey を自動選択して local storage 確認を完了する。
#[test]
fn verify_yubikey_auto_selects_single_detected_device() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["verify-yubikey"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"name\": \"local-storage\""));
    assert!(stdout.contains("\"status\": \"ok\""));
    assert!(!stdout.contains("\"serial\""));
    Ok(())
}

/// `verify-yubikey` は `--all` と個別 `--check` の同時指定を device 解決前に拒否する。
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

/// `bw-login` は YubiKey 由来 credential と stdin OTP で session key だけを surface する。
#[test]
fn bw_login_reads_yubikey_secrets_and_surfaces_session() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    // OTP は単回トークンとして stdin pipe（非 TTY）から 1 行で読む。
    let run = run_pipe_with_stub(["bw-login"], Some("ccccbtdvotp\n"), &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    // stdout は単一 JSON として機械可読に保ち、session key を含める。master password は出力しない。
    assert!(stdout.contains("\"bw_login\": \"ok\""));
    assert!(stdout.contains("\"bw_session\": \"STUBSESSIONKEY==\""));
    // 利用者が export できるヒント行は stderr に出し、stdout の JSON 機械可読性を保つ。
    assert!(
        !stdout.contains("export BW_SESSION="),
        "export hint must not break stdout JSON: {stdout}"
    );
    assert!(run.stderr.contains("export BW_SESSION='STUBSESSIONKEY=='"));
    assert!(
        !stdout.contains("pw"),
        "master password must not be surfaced"
    );
    assert!(
        !run.stderr.contains("pw"),
        "master password must not be surfaced"
    );
    // stub observation: YubiKey の bw-email と入力 OTP を観測し、unlock 済みになる。
    let final_bw_login = final_observation(&run.stdout, "bw-login")?;
    assert_eq!(final_bw_login["observed_email"], json!("u@example.com"));
    assert_eq!(final_bw_login["observed_otp"], json!("ccccbtdvotp"));
    assert_eq!(final_bw_login["unlocked"], json!(true));
    Ok(())
}

/// `bw-login --email` は YubiKey の stored email ではなく override email を login 境界へ渡す。
#[test]
fn bw_login_uses_email_override() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(
        ["bw-login", "--email", "override@example.com"],
        Some("ccccbtdvotp\n"),
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    // override 指定時は YubiKey の bw-email ではなく override が login email として使われる。
    let final_bw_login = final_observation(&run.stdout, "bw-login")?;
    assert_eq!(
        final_bw_login["observed_email"],
        json!("override@example.com")
    );
    assert_eq!(final_bw_login["unlocked"], json!(true));
    Ok(())
}

/// 削除済みの `--serial` を `bw-login` で受け付けない CLI 境界を固定する。
#[test]
fn bw_login_rejects_removed_serial_option() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["bw-login", "--serial", "2001"], Some("otp\n"), &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("unexpected argument '--serial'"));
    Ok(())
}

/// `bw-login` は master password 不一致時に session key を stdout へ出さず失敗する。
#[test]
fn bw_login_fails_when_master_password_does_not_match() -> TestResult<()> {
    // stub の expected_password を YubiKey 値（"pw"）と不一致にして login 失敗を再現する。
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    )
    .with_bw_login(json!({
        "expected_password": "different",
        "session_key": "STUBSESSIONKEY=="
    }));
    let run = run_pipe_with_stub(["bw-login"], Some("ccccbtdvotp\n"), &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    // 失敗時は session を surface しない。
    assert!(!run.user_stdout().contains("export BW_SESSION="));
    Ok(())
}

/// `verify-yubikey --check bw-login` は bw-login 外部確認を実行し、unlock 済み状態を観測する。
#[test]
fn verify_yubikey_runs_bw_login_external_check() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(
        ["verify-yubikey", "--check", "bw-login"],
        Some("ccccbtdvotp\n"),
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"name\": \"bw-login\""));
    assert!(stdout.contains("\"status\": \"ok\""));
    let final_bw_login = final_observation(&run.stdout, "bw-login")?;
    assert_eq!(final_bw_login["unlocked"], json!(true));
    Ok(())
}

/// `verify-yubikey --check bw-login --email` は override email で bw-login 確認を行う。
#[test]
fn verify_yubikey_bw_login_check_uses_email_override() -> TestResult<()> {
    // `--check bw-login --email <override>` は override email で bw-login 確認を行い、YubiKey の bw-email
    // （"u@example.com"）を login email に使わない（secret-recovery-spec.md の `dotfiles secrets bw-login` 節）。
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(
        [
            "verify-yubikey",
            "--check",
            "bw-login",
            "--email",
            "override@example.com",
        ],
        Some("ccccbtdvotp\n"),
        &stub,
    )?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    assert!(stdout.contains("\"name\": \"bw-login\""));
    assert!(stdout.contains("\"status\": \"ok\""));
    let final_bw_login = final_observation(&run.stdout, "bw-login")?;
    assert_eq!(
        final_bw_login["observed_email"],
        json!("override@example.com")
    );
    assert_eq!(final_bw_login["unlocked"], json!(true));
    Ok(())
}

/// `verify-yubikey --all` は BWS と bw-login の外部確認を同じ verify 実行に含める。
#[test]
fn verify_yubikey_all_includes_bw_login_and_bws_checks() -> TestResult<()> {
    let envelope = restore_envelope_json(PRIMARY_SERIAL);
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_backup_and_pass_remote(&envelope, RESTORE_PASS_REMOTE),
    );
    let run = run_pipe_with_stub(["verify-yubikey", "--all"], Some("ccccbtdvotp\n"), &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    // `--all` は bws と bw-login の両方の外部確認を含む（spec L107）。
    assert!(stdout.contains("\"name\": \"bws\""));
    assert!(stdout.contains("\"name\": \"bw-login\""));
    assert!(!stdout.contains("\"status\": \"skipped\""));
    assert!(!stdout.contains("\"status\": \"failed\""));
    Ok(())
}

/// `verify-yubikey` の引数なし実行では bw-login 外部確認を skipped として要約に残す。
#[test]
fn verify_yubikey_no_args_leaves_bw_login_skipped() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["verify-yubikey"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    // 引数なし実行では bw-login 外部確認は machine-readable な skipped として残す（spec L155）。
    assert!(stdout.contains("\"name\": \"bw-login\""));
    assert!(stdout.contains("\"status\": \"skipped\""));
    Ok(())
}

/// `put --stdin` は device 解決後の secret 入力欠落を、複数接続エラーとは別の失敗として返す。
#[test]
fn put_stdin_rejects_missing_secret_input_after_device_resolution() -> TestResult<()> {
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
    assert!(!run.stderr.contains("connect exactly one YubiKey"));
    Ok(())
}

/// internal stub build では env 未設定時に stub 経路であることを明示して停止する。
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

/// device policy が PIN を要求する場合、verify は PIN 未入力のまま成功しない。
#[test]
fn verify_yubikey_requires_pin_when_device_policy_demands_it() -> TestResult<()> {
    let initial = yubikey_spec_requiring_pin([provisioned_device_spec(PRIMARY_SERIAL)]);
    let stub = StubPorts::new(initial, bws_spec());
    let run = run_pipe_with_stub(["verify-yubikey"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr.contains("PIN") || run.stderr.contains("pin"),
        "stderr: {}",
        run.stderr
    );
    Ok(())
}

/// `yubikey put` の保存結果は YubiKey stub の最終 datastore 観測に反映される。
#[test]
fn put_updates_final_yubikey_spec_with_yubikey_path() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([writable_bws_access_token_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let put_run = run_pipe_with_stub(
        ["yubikey", "put", "bws-access-token", "--stdin"],
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

/// `yubikey get` は seed 済み storage から対象 secret を読み出す。
#[test]
fn get_reads_seeded_secret_with_yubikey_path() -> TestResult<()> {
    let initial_device =
        seeded_device_spec(PRIMARY_SERIAL, "seed@example.com", "seed-pw", "seed-token");
    let stub = StubPorts::new(yubikey_spec([initial_device]), bws_spec());
    let run = run_pipe_with_stub(["yubikey", "get", "bws-access-token"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    assert_eq!(run.user_stdout(), "seed-token");
    Ok(())
}

/// `yubikey get` は対象 secret の保存 blob が壊れている場合に decode error で停止する。
#[test]
fn get_fails_when_storage_is_corrupt_with_yubikey_path() -> TestResult<()> {
    let initial_device = storage_decode_error_device_spec(
        provisioned_device_spec(PRIMARY_SERIAL),
        "bws-access-token",
    );
    let stub = StubPorts::new(yubikey_spec([initial_device]), bws_spec());
    let run = run_pipe_with_stub(["yubikey", "get", "bws-access-token"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to decode bws-access-token"));
    Ok(())
}

/// `rotate-bws-token` は既存 storage の decode error を上書きで隠さず停止する。
#[test]
fn rotate_fails_when_seeded_storage_is_corrupt_with_yubikey_path() -> TestResult<()> {
    let initial_device =
        storage_decode_error_device_spec(provisioned_device_spec(PRIMARY_SERIAL), "bw-password");
    let stub = StubPorts::new(yubikey_spec([initial_device]), bws_spec());
    let run = run_pipe_with_stub(
        ["yubikey", "rotate-bws-token", "--stdin"],
        Some("new-token\r"),
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("failed to decode bw-password"));
    Ok(())
}

/// `verify-yubikey` は seed 済み storage の decode error を failed 状態として返す。
#[test]
fn verify_fails_when_seeded_storage_is_corrupt_with_yubikey_path() -> TestResult<()> {
    let initial_device =
        storage_decode_error_device_spec(provisioned_device_spec(PRIMARY_SERIAL), "bw-email");
    let stub = StubPorts::new(yubikey_spec([initial_device]), bws_spec());
    let run = run_pipe_with_stub(["verify-yubikey"], None, &stub)?;

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
        .map_err(|_| PtyOutputReaderJoinFailed)??;
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

/// 既定 bw-login stub spec。provisioned fixture の `bw-password`（"pw"）と一致した場合だけ login 成功とし、
/// 成功時に固定 session key を返す。
fn default_bw_login_spec() -> Value {
    json!({
        "expected_password": "pw",
        "session_key": "STUBSESSIONKEY=="
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
    let mut recipients = vec![json!({
        "yubikey_serial": serial.to_string(),
        "piv_slot": "82",
        "public_key_fingerprint": pubkey,
        "wrapped_dek": "d3JhcHBlZA=="
    })];
    if serial == PRIMARY_SERIAL {
        recipients.push(json!({
            "yubikey_serial": SPARE_SERIAL.to_string(),
            "piv_slot": "82",
            "public_key_fingerprint": stub_recipient_fingerprint(SPARE_SERIAL),
            "wrapped_dek": "c3BhcmUtd3JhcHBlZA=="
        }));
    }
    json!({
        "version": 1,
        "metadata": {
            "primary_fingerprint": RESTORE_PRIMARY_FP,
            "exported_at": "2026-05-31T00:00:00Z",
            "dek_alg": "aes-256-gcm",
            "recipient_kek_alg": "rsa-oaep-sha256"
        },
        "recipients": recipients,
        "ciphertext": {
            "nonce": "EBESExQVFhcYGRob",
            "body": "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWYwMTIzNDU2Nw==",
            "tag": "gIGCg4SFhoeIiYqLjI2Ojw=="
        }
    })
    .to_string()
}

/// 指定 serial の recipient 1 件だけを持つ encrypted envelope JSON を作る。
fn restore_single_recipient_envelope_json(serial: u32) -> String {
    let pubkey = stub_recipient_fingerprint(serial);
    json!({
        "version": 1,
        "metadata": {
            "primary_fingerprint": RESTORE_PRIMARY_FP,
            "exported_at": "2026-05-31T00:00:00Z",
            "dek_alg": "aes-256-gcm",
            "recipient_kek_alg": "rsa-oaep-sha256"
        },
        "recipients": [{
            "yubikey_serial": serial.to_string(),
            "piv_slot": "82",
            "public_key_fingerprint": pubkey,
            "wrapped_dek": "d3JhcHBlZA=="
        }],
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

/// password-store-remote note を override した BWS spec を作る。
fn bws_spec_with_pass_remote_note(remote: &str, note: Option<&str>) -> Value {
    let mut spec = json!({
        "fixture": "default-recovery-project",
        "password_store_remote": remote
    });
    if let Some(note) = note {
        spec["password_store_remote_note"] = json!(note);
    }
    spec
}

/// 復旧 project と access token はあるが `password-store-remote` は未登録の BWS spec を作る。
fn bws_spec_without_pass_remote() -> Value {
    json!({
        "fixture": "default-recovery-project",
        "password_store_remote_absent": true
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

/// `restore-pass` は BWS の remote URL を使って password-store を clone し、復号可読性まで確認する。
#[test]
fn restore_pass_clones_store_and_confirms_readability_with_stub_paths() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_pass_remote(RESTORE_PASS_REMOTE),
    )
    .with_gpg(gpg_spec_for_restore_pass());
    let run = run_pipe_with_stub(["restore-pass"], None, &stub)?;

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

/// 削除済みの `--serial` を `restore-pass` で受け付けない CLI 境界を固定する。
#[test]
fn restore_pass_rejects_removed_serial_option() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_pass_remote(RESTORE_PASS_REMOTE),
    );
    let run = run_pipe_with_stub(["restore-pass", "--serial", "2001"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("unexpected argument '--serial'"));
    Ok(())
}

/// `restore-pass` は既存 password-store がある場合に clone へ進まず停止する。
#[test]
fn restore_pass_stops_when_store_already_exists_with_stub_paths() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_pass_remote(RESTORE_PASS_REMOTE),
    )
    .with_git(git_spec_with_existing_store());
    let run = run_pipe_with_stub(["restore-pass"], None, &stub)?;

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

/// `restore-pass` は BWS から取得した remote URL が domain 制約を満たさない場合に停止する。
#[test]
fn restore_pass_fails_when_remote_url_is_invalid_with_stub_paths() -> TestResult<()> {
    // 既定 fixture の password-store-remote は `https://example.invalid/repo.git` で domain 妥当でない。
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec(),
    );
    let run = run_pipe_with_stub(["restore-pass"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr.contains("password-store-remote"),
        "stderr: {}",
        run.stderr
    );
    Ok(())
}

/// `restore-pass` は clone 後 store entry を GPG で復号できない場合に停止する。
#[test]
fn restore_pass_fails_when_cloned_store_is_unreadable_with_stub_paths() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_pass_remote(RESTORE_PASS_REMOTE),
    )
    .with_gpg(gpg_spec_for_restore_pass())
    .with_git(git_spec_with_unreadable_store());
    let run = run_pipe_with_stub(["restore-pass"], None, &stub)?;

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

/// `restore-pass` は `.gpg-id` recipient に対応する秘密鍵がない場合に復旧不能として停止する。
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
    let run = run_pipe_with_stub(["restore-pass"], None, &stub)?;

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

/// 既存 `password-store-remote` があっても configured origin が無ければ fail-closed で停止する。
#[test]
fn pass_remote_register_stops_without_configured_origin_for_existing_secret() -> TestResult<()> {
    let initial = bws_spec_with_pass_remote(RESTORE_PASS_REMOTE);
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        initial,
    );
    let run = run_pipe_with_stub(["pass-remote", "register"], Some("token\n"), &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert_eq!(
        run.stderr.trim(),
        "existing password-store-remote cannot be reused without a configured local origin"
    );
    Ok(())
}

/// `password-store-remote` 未登録時は local origin から導出し、URL を argv/stdin 経由で再入力させない。
#[test]
fn pass_remote_register_uses_configured_origin_without_url_input() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_without_pass_remote(),
    )
    .with_git(json!({
        "configured_origin_remote": RESTORE_PASS_REMOTE
    }));
    let run = run_pipe_with_stub(["pass-remote", "register"], Some("token\n"), &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    let final_bws = run.final_bws()?;
    assert_eq!(
        final_bws["resolved_secrets"]["password-store-remote"],
        json!(RESTORE_PASS_REMOTE),
        "configured local origin must seed password-store-remote without URL argv/stdin input"
    );
    Ok(())
}

/// origin 不在時は stdin の token を URL 入力へ再利用せず、controlling TTY の可視入力だけを要求する。
#[test]
fn pass_remote_register_does_not_reuse_stdin_as_url_without_configured_origin() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_without_pass_remote(),
    );
    let run = run_pipe_with_stub(["pass-remote", "register"], Some("token\n"), &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr.contains("failed to open controlling terminal"),
        "stderr: {}",
        run.stderr
    );
    let final_bws = run.final_bws()?;
    assert_eq!(
        final_bws["resolved_secrets"].get("password-store-remote"),
        None,
        "stdin token must not be reinterpreted as password-store-remote URL input"
    );
    Ok(())
}

/// 空の BWS 登録用 token は BWS project 解決開始後、BWS 側 token 検証で停止し、`bw login` 相当へ進めない。
#[test]
fn pass_remote_register_rejects_empty_piped_bws_token_before_any_bws_login() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_without_pass_remote(),
    )
    .with_git(json!({
        "configured_origin_remote": RESTORE_PASS_REMOTE
    }));
    let run = run_pipe_with_stub(["pass-remote", "register"], Some("\n"), &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr.contains(
            "`pass-remote register` failed while resolving BWS project `dotfiles-secret-recovery`"
        ),
        "stderr: {}",
        run.stderr
    );
    assert!(
        run.stderr.contains(
            "caused by:\n  1: BWS internal stub failed to list projects\n  2: bws access token must not be empty"
        ),
        "stderr: {}",
        run.stderr
    );
    assert!(
        !run.has_bws_observation(),
        "empty token must stop before BWS observation is written: {}",
        run.stdout
    );
    Ok(())
}

/// TTY prompt 経路でも空の BWS 登録用 token は BWS project 解決開始後の token 検証で停止する。
#[test]
fn pass_remote_register_rejects_empty_tty_bws_token_before_any_bws_login() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_without_pass_remote(),
    )
    .with_git(json!({
        "configured_origin_remote": RESTORE_PASS_REMOTE
    }));
    let run = run_pty_with_stub(["pass-remote", "register"], Some("\n"), &stub)?;

    assert!(!run.success, "output: {}", run.output);
    assert!(
        run.output.contains("bws-access-token (create/use): "),
        "output: {}",
        run.output
    );
    assert!(
        run.output.contains(
            "`pass-remote register` failed while resolving BWS project `dotfiles-secret-recovery`"
        ),
        "output: {}",
        run.output
    );
    assert!(
        run.output.contains(
            "caused by:\r\n  1: BWS internal stub failed to list projects\r\n  2: bws access token must not be empty"
        ),
        "output: {}",
        run.output
    );
    assert!(
        !run.output
            .contains("BWS internal stub rejected the provided access token"),
        "empty token must not fall through to BWS backend auth failure: {}",
        run.output
    );
    assert!(
        !run.output.contains(STUB_OBSERVATION_PREFIX),
        "empty token must stop before BWS observation is written: {}",
        run.output
    );
    Ok(())
}

/// 非空だが無効な BWS 登録用 token は、`pass-remote register` の operation / backend / source chain を表示する。
#[test]
fn pass_remote_register_surfaces_operation_backend_and_source_chain_for_bws_auth_failure()
-> TestResult<()> {
    let mut bws = bws_spec_without_pass_remote();
    bws["force_auth_failure"] = json!(true);
    let stub = StubPorts::new(yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]), bws)
        .with_git(json!({
            "configured_origin_remote": RESTORE_PASS_REMOTE
        }));
    let run = run_pipe_with_stub(["pass-remote", "register"], Some("token\n"), &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr.contains(
            "`pass-remote register` failed while resolving BWS project `dotfiles-secret-recovery`"
        ),
        "stderr: {}",
        run.stderr
    );
    assert!(
        run.stderr
            .contains("  1: BWS internal stub failed to list projects"),
        "stderr: {}",
        run.stderr
    );
    assert!(
        run.stderr
            .contains("  2: BWS internal stub rejected the provided access token"),
        "stderr: {}",
        run.stderr
    );
    assert!(
        !run.stderr.contains("bitwarden login failed"),
        "legacy top-level message must not survive without chain context: {}",
        run.stderr
    );
    assert!(
        !run.has_bws_observation(),
        "auth failure must stop before BWS observation is written: {}",
        run.stdout
    );
    Ok(())
}

/// 削除済みの `--url` を `pass-remote register` で受け付けない CLI 境界を固定する。
#[test]
fn pass_remote_register_stops_when_input_url_is_invalid() -> TestResult<()> {
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_without_pass_remote(),
    );
    let run = run_pty_with_stub(
        ["pass-remote", "register", "--url", "not-a-valid-clone-url"],
        Some("token\n"),
        &stub,
    )?;

    assert!(!run.success, "output: {}", run.output);
    assert!(
        run.output.contains("unexpected argument '--url'"),
        "output: {}",
        run.output
    );
    Ok(())
}

/// 既存 `password-store-remote` が configured origin と不一致なら stale 値として停止する。
#[test]
fn pass_remote_register_fails_closed_when_existing_secret_mismatches_origin() -> TestResult<()> {
    let initial = bws_spec_with_pass_remote(RESTORE_PASS_REMOTE);
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        initial,
    )
    .with_git(json!({
        "configured_origin_remote": "git@github.com:owner/other-store.git"
    }));
    let run = run_pipe_with_stub(["pass-remote", "register"], Some("token\n"), &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr
            .contains("does not match the configured local origin"),
        "stderr: {}",
        run.stderr
    );
    Ok(())
}

/// `restore-gpg` は BWS envelope を復号して GPG import と SSH support 登録まで完了する。
#[test]
fn restore_gpg_imports_key_and_registers_ssh_with_stub_paths() -> TestResult<()> {
    let envelope = restore_envelope_json(PRIMARY_SERIAL);
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_backup(&envelope),
    )
    .with_gpg(gpg_spec_with_importable_key());
    let run = run_pipe_with_stub(["restore-gpg"], None, &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    let stdout = run.user_stdout();
    assert!(!stdout.contains("primary_fingerprint"), "stdout: {stdout}");
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

/// 削除済みの `--serial` を `restore-gpg` で受け付けない CLI 境界を固定する。
#[test]
fn restore_gpg_rejects_removed_serial_option() -> TestResult<()> {
    let envelope = restore_envelope_json(PRIMARY_SERIAL);
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_backup(&envelope),
    );
    let run = run_pipe_with_stub(["restore-gpg", "--serial", "2001"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("unexpected argument '--serial'"));
    Ok(())
}

/// `restore-gpg` は同一 primary key が既に存在する場合に import へ進まず停止する。
#[test]
fn restore_gpg_stops_when_existing_key_collides_with_stub_paths() -> TestResult<()> {
    let envelope = restore_envelope_json(PRIMARY_SERIAL);
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_backup(&envelope),
    )
    .with_gpg(gpg_spec_with_existing_key());
    let run = run_pipe_with_stub(["restore-gpg"], None, &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr.contains("already exists"),
        "stderr: {}",
        run.stderr
    );
    Ok(())
}

/// top-level `gpg export-ssh-public-key` は復元済み primary から OpenSSH 公開鍵 1 行を出力する。
#[test]
fn export_ssh_public_key_writes_openssh_line_with_stub_paths() -> TestResult<()> {
    let stub =
        StubPorts::new(yubikey_spec([]), bws_spec()).with_gpg(gpg_spec_with_importable_key());
    // `dotfiles gpg export-ssh-public-key` は top-level command なので secrets 経由ではない。
    let mut command = Command::new(env!("CARGO_BIN_EXE_dotfiles"));
    command
        .arg("gpg")
        .arg("export-ssh-public-key")
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

/// 削除済みの `--primary-fingerprint` を `gpg export-ssh-public-key` で受け付けない CLI 境界を固定する。
#[test]
fn export_ssh_public_key_rejects_removed_primary_fingerprint_option() -> TestResult<()> {
    let stub =
        StubPorts::new(yubikey_spec([]), bws_spec()).with_gpg(gpg_spec_with_importable_key());
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

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr
            .contains("unexpected argument '--primary-fingerprint'")
    );
    Ok(())
}

/// `dotfiles gpg export-ssh-public-key` は `--primary-fingerprint` 省略時に `.gpg-id` recipient を優先する。
#[test]
fn export_ssh_public_key_without_fingerprint_prefers_gpg_id_with_stub_paths() -> TestResult<()> {
    let stub = StubPorts::new(yubikey_spec([]), bws_spec())
        .with_gpg(json!({
            "existing_keys": [],
            "keys": {
                RESTORE_PRIMARY_FP: {
                    "capabilities": ["encryption", "authentication", "signing"],
                    "keygrip": RESTORE_KEYGRIP,
                    "ssh_public_key": RESTORE_SSH_LINE
                }
            },
            "held_recipients": [RESTORE_PASS_RECIPIENT]
        }))
        .with_git(json!({
            "store_exists": true,
            "gpg_id_present": true,
            "gpg_id_recipients": [RESTORE_PASS_RECIPIENT]
        }));
    let mut command = Command::new(env!("CARGO_BIN_EXE_dotfiles"));
    command
        .arg("gpg")
        .arg("export-ssh-public-key")
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

/// `dotfiles secrets gpg-backup register` は `--primary-fingerprint` 省略時に `.gpg-id` recipient を使って
/// 既存 2 recipient envelope の照合へ入る。
#[test]
fn gpg_backup_register_without_fingerprint_uses_gpg_id_with_stub_paths() -> TestResult<()> {
    let envelope = restore_envelope_json(PRIMARY_SERIAL);
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_backup(&envelope),
    )
    .with_gpg(json!({
        "existing_keys": [],
        "keys": {
            RESTORE_PRIMARY_FP: {
                "capabilities": ["encryption", "authentication", "signing"],
                "keygrip": RESTORE_KEYGRIP,
                "ssh_public_key": RESTORE_SSH_LINE
            }
        },
        "held_recipients": [RESTORE_PASS_RECIPIENT]
    }))
    .with_git(json!({
        "store_exists": true,
        "gpg_id_present": true,
        "gpg_id_recipients": [RESTORE_PASS_RECIPIENT]
    }));
    let run = run_pipe_with_stub(["gpg-backup", "register"], Some("token\n"), &stub)?;

    assert!(run.success, "stderr: {}", run.stderr);
    Ok(())
}

/// `gpg-backup register` は BWS auth failure 時に operation / backend / source chain を維持する。
#[test]
fn gpg_backup_register_surfaces_operation_backend_and_source_chain_for_bws_auth_failure()
-> TestResult<()> {
    let mut bws = bws_spec_with_backup(&restore_envelope_json(PRIMARY_SERIAL));
    bws["force_auth_failure"] = json!(true);
    let stub = StubPorts::new(yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]), bws)
        .with_gpg(json!({
            "existing_keys": [],
            "keys": {
                RESTORE_PRIMARY_FP: {
                    "capabilities": ["encryption", "authentication", "signing"],
                    "keygrip": RESTORE_KEYGRIP,
                    "ssh_public_key": RESTORE_SSH_LINE
                }
            },
            "held_recipients": [RESTORE_PASS_RECIPIENT]
        }))
        .with_git(json!({
            "store_exists": true,
            "gpg_id_present": true,
            "gpg_id_recipients": [RESTORE_PASS_RECIPIENT]
        }));
    let run = run_pipe_with_stub(["gpg-backup", "register"], Some("token\n"), &stub)?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr.contains(
            "`gpg-backup register` failed while resolving BWS project `dotfiles-secret-recovery`"
        ),
        "stderr: {}",
        run.stderr
    );
    assert!(
        run.stderr.contains(
            "caused by:\n  1: BWS internal stub failed to list projects\n  2: BWS internal stub rejected the provided access token"
        ),
        "stderr: {}",
        run.stderr
    );
    assert!(
        !run.stderr.contains("bitwarden login failed"),
        "legacy top-level message must not survive without chain context: {}",
        run.stderr
    );
    assert!(
        !run.has_bws_observation(),
        "auth failure must stop before BWS observation is written: {}",
        run.stdout
    );
    Ok(())
}

/// 削除済みの `--primary-fingerprint` を `gpg-backup register` で受け付けない CLI 境界を固定する。
#[test]
fn gpg_backup_register_rejects_removed_primary_fingerprint_option() -> TestResult<()> {
    let envelope = restore_envelope_json(PRIMARY_SERIAL);
    let stub = StubPorts::new(
        yubikey_spec([provisioned_device_spec(PRIMARY_SERIAL)]),
        bws_spec_with_backup(&envelope),
    );
    let run = run_pipe_with_stub(
        [
            "gpg-backup",
            "register",
            "--primary-fingerprint",
            RESTORE_PRIMARY_FP,
        ],
        Some("token\n"),
        &stub,
    )?;

    assert!(!run.success, "stdout: {}", run.stdout);
    assert!(
        run.stderr
            .contains("unexpected argument '--primary-fingerprint'"),
        "stderr: {}",
        run.stderr
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
