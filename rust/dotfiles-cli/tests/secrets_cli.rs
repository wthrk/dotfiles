#![cfg(feature = "secrets-internal-test-stub")]

//! `dotfiles secrets` の公開 CLI 面と internal stub 経由の秘密値非露出を検証する integration test。

use std::{
    io::Write,
    process::{Command, Output, Stdio},
};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

fn dotfiles(args: impl IntoIterator<Item = &'static str>) -> TestResult<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_dotfiles"))
        .args(args)
        .output()?)
}

fn dotfiles_with_env(
    args: impl IntoIterator<Item = &'static str>,
    envs: impl IntoIterator<Item = (&'static str, String)>,
) -> TestResult<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_dotfiles"))
        .args(args)
        .envs(envs)
        .output()?)
}

fn dotfiles_with_stdin(
    args: impl IntoIterator<Item = &'static str>,
    stdin_payload: &'static [u8],
) -> TestResult<Output> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_dotfiles"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(stdin_payload)?;
    }
    Ok(child.wait_with_output()?)
}

fn combined_output(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_no_leaks(text: &str, context: &str, forbidden: &[(&str, &str)]) {
    for (label, leaked) in forbidden {
        assert!(
            !text.contains(leaked),
            "{context} must not leak fixture value category `{label}`"
        );
    }
}

fn assert_no_primary_fingerprint_fixture_leaks(text: &str, context: &str) {
    assert_no_leaks(
        text,
        context,
        &[
            (
                "primary fingerprint full",
                "0123456789abcdef0123456789abcdef01234567",
            ),
            ("primary fingerprint prefix16", "0123456789abcdef"),
            ("primary fingerprint prefix8", "01234567"),
            ("primary fingerprint display", "...4567"),
        ],
    );
}

fn assert_no_mismatched_fingerprint_fixture_leaks(text: &str, context: &str) {
    assert_no_leaks(
        text,
        context,
        &[
            (
                "mismatched fingerprint full",
                "ffffffffffffffffffffffffffffffffffffffff",
            ),
            ("mismatched fingerprint prefix16", "ffffffffffffffff"),
            ("mismatched fingerprint prefix8", "ffffffff"),
            ("mismatched fingerprint display", "...ffff"),
        ],
    );
}

fn assert_serial_option_rejected(args: &[&'static str], context: &str) -> TestResult<()> {
    let mut command = args.to_vec();
    command.extend(["--serial", "12345678"]);
    let output = dotfiles(command)?;
    assert!(!output.status.success(), "{context} must reject --serial");

    let text = combined_output(&output);
    assert!(
        text.contains("unexpected argument") || text.contains("unrecognized"),
        "{context} must reject --serial at the clap boundary"
    );
    assert_no_leaks(&text, context, &[("serial fixture", "12345678")]);
    Ok(())
}

#[test]
/// secrets help が撤去済みの管理系語彙や CLI 経路を再公開しないことを固定する。
fn secrets_help_does_not_expose_removed_manager_or_cli_paths() -> TestResult<()> {
    let output = dotfiles(["secrets", "--help"])?;
    assert!(output.status.success(), "help must render successfully");

    let text = combined_output(&output);
    for forbidden in [
        "Bitwarden Secrets Manager",
        "Secrets Manager",
        "BWS",
        "bws-access-token",
        "bw-login",
        "bw login",
        "bw unlock",
        "BW_SESSION",
        "project",
        "organization",
    ] {
        assert!(
            !text.contains(forbidden),
            "help must not contain removed management/session term"
        );
    }
    Ok(())
}

#[test]
/// `bws-access-token` が YubiKey 保存対象として復活しないことを clap/domain 境界で固定する。
fn yubikey_put_rejects_bws_access_token_secret_name() -> TestResult<()> {
    let output = dotfiles(["secrets", "yubikey", "put", "bws-access-token"])?;
    assert!(
        !output.status.success(),
        "bws-access-token must not be accepted as a YubiKey secret name"
    );

    let text = combined_output(&output);
    assert!(
        text.contains("unsupported YubiKey secret name"),
        "unsupported storage name should be rejected at the CLI boundary"
    );
    assert!(
        !text.contains("Bitwarden Secrets Manager")
            && !text.contains("project")
            && !text.contains("organization"),
        "rejection must not reintroduce BWS/project/organization guidance"
    );
    Ok(())
}

#[test]
/// 不正な YubiKey secret name は入力値を error に echo しない。
fn yubikey_put_rejects_unknown_secret_name_without_echoing_input() -> TestResult<()> {
    let output = dotfiles([
        "secrets",
        "yubikey",
        "put",
        "https://example.invalid/0123456789abcdef0123456789abcdef01234567",
    ])?;
    assert!(
        !output.status.success(),
        "unsupported secret name must fail"
    );

    let text = combined_output(&output);
    assert!(
        text.contains("unsupported YubiKey secret name"),
        "unsupported storage name should be rejected with a fixed message"
    );
    assert_no_leaks(
        &text,
        "unsupported secret name error",
        &[("unsupported secret name domain", "example.invalid")],
    );
    assert_no_primary_fingerprint_fixture_leaks(&text, "unsupported secret name error");
    Ok(())
}

#[test]
/// 不正な YubiKey get secret name も入力値を error に echo しない。
fn yubikey_get_rejects_unknown_secret_name_without_echoing_input() -> TestResult<()> {
    let output = dotfiles([
        "secrets",
        "yubikey",
        "get",
        "https://example.invalid/0123456789abcdef0123456789abcdef01234567",
    ])?;
    assert!(
        !output.status.success(),
        "unsupported secret name must fail"
    );

    let text = combined_output(&output);
    assert!(
        text.contains("unsupported YubiKey secret name"),
        "unsupported storage name should be rejected with a fixed message"
    );
    assert_no_leaks(
        &text,
        "unsupported get secret name error",
        &[("unsupported secret name domain", "example.invalid")],
    );
    assert_no_primary_fingerprint_fixture_leaks(&text, "unsupported get secret name error");
    Ok(())
}

#[test]
/// 削除済み `bw-login` command が clap 境界で拒否されることを固定する。
fn bw_login_command_is_removed() -> TestResult<()> {
    let output = dotfiles(["secrets", "bw-login"])?;
    assert!(
        !output.status.success(),
        "removed bw-login command must fail"
    );

    let text = combined_output(&output);
    assert!(
        text.contains("unrecognized subcommand") || text.contains("invalid"),
        "clap should reject removed bw-login command"
    );
    assert!(
        !text.contains("BW_SESSION") && !text.contains("bw unlock") && !text.contains("bw login"),
        "removed command must not surface bw CLI/session details"
    );
    Ok(())
}

#[test]
/// `verify-yubikey` help が個人 vault check だけを公開することを固定する。
fn verify_yubikey_help_only_exposes_personal_vault_check() -> TestResult<()> {
    let output = dotfiles(["secrets", "verify-yubikey", "--help"])?;
    assert!(
        output.status.success(),
        "verify help must render successfully"
    );

    let text = combined_output(&output);
    assert!(text.contains("vault"), "vault check must be documented");
    for forbidden in [
        "BWS",
        "bws-access-token",
        "bw-login",
        "BW_SESSION",
        "project",
        "organization",
    ] {
        assert!(
            !text.contains(forbidden),
            "verify help must not contain removed management/session term"
        );
    }
    Ok(())
}

#[test]
/// 低水準 YubiKey put command が stdin secret 入力 option を公開しないことを固定する。
fn yubikey_put_help_does_not_expose_stdin_secret_input() -> TestResult<()> {
    let output = dotfiles(["secrets", "yubikey", "put", "--help"])?;
    assert!(output.status.success(), "put help must render successfully");

    let text = combined_output(&output);
    assert!(
        !text.contains("--stdin"),
        "put help must not expose stdin secret input"
    );
    Ok(())
}

#[test]
/// 削除済み `--stdin` option が clap 境界で拒否され、stdin payload を処理しないことを固定する。
fn yubikey_put_stdin_secret_input_option_is_rejected() -> TestResult<()> {
    let output = dotfiles_with_stdin(
        [
            "secrets",
            "yubikey",
            "put",
            "bitwarden-client-id",
            "--stdin",
        ],
        b"stdin-secret-payload-must-not-be-processed\n",
    )?;
    assert!(!output.status.success(), "removed --stdin option must fail");

    let text = combined_output(&output);
    assert!(
        text.contains("unexpected argument") || text.contains("unrecognized"),
        "clap should reject removed --stdin option"
    );
    assert!(
        !text.contains("stdin-secret-payload-must-not-be-processed"),
        "removed --stdin path must not echo or process stdin payload"
    );
    Ok(())
}

#[test]
/// secret-recovery の公開 command が YubiKey serial 指定 option を受け付けないことを固定する。
fn secret_recovery_commands_reject_serial_option() -> TestResult<()> {
    assert_serial_option_rejected(&["secrets", "yubikey", "enroll-primary"], "enroll-primary")?;
    assert_serial_option_rejected(&["secrets", "restore-gpg"], "restore-gpg")?;
    assert_serial_option_rejected(
        &["secrets", "gpg-backup", "register"],
        "gpg-backup register",
    )?;
    Ok(())
}

#[test]
/// `pass-remote register` が個人 vault adapter 境界を使い、URL 実値を出力しないことを検証する。
fn pass_remote_register_creates_personal_vault_item_with_internal_stub() -> TestResult<()> {
    let output = dotfiles_with_env(
        ["secrets", "pass-remote", "register"],
        [
            yubikey_stub_env(),
            ("DOTFILES_SECRETS_VAULT_STUB_SPEC_JSON", r#"{"secrets":{}}"#.to_owned()),
            (
                "DOTFILES_SECRETS_GIT_STUB_SPEC_JSON",
                r#"{"store_exists":true,"configured_origin_remote":"https://github.com/example-owner/password-store"}"#.to_owned(),
            ),
        ],
    )?;

    let text = combined_output(&output);
    assert!(
        output.status.success(),
        "pass-remote register should use personal vault stub"
    );
    assert!(
        text.contains(r#""port":"bitwarden-vault""#)
            && text.contains(r#""password-store-remote":"<redacted>""#),
        "vault stub should observe redacted created item"
    );
    assert!(
        !text.contains("example-owner/password-store"),
        "clone URL must not be echoed through CLI output"
    );
    Ok(())
}

#[test]
/// vault 認証失敗時も error chain を保ち、secret と URL fixture 値を露出しないことを検証する。
fn pass_remote_register_failure_renders_error_chain_without_secret_values() -> TestResult<()> {
    let output = dotfiles_with_env(
        ["secrets", "pass-remote", "register"],
        [
            yubikey_stub_env(),
            (
                "DOTFILES_SECRETS_VAULT_STUB_SPEC_JSON",
                r#"{"auth_fails":true}"#.to_owned(),
            ),
            (
                "DOTFILES_SECRETS_GIT_STUB_SPEC_JSON",
                r#"{"store_exists":true,"configured_origin_remote":"https://github.com/example-owner/password-store"}"#.to_owned(),
            ),
        ],
    )?;

    let text = combined_output(&output);
    assert!(!output.status.success(), "vault auth failure should fail");
    assert!(
        text.contains("caused by:"),
        "CLI must render the anyhow source chain"
    );
    assert!(
        text.contains("Bitwarden vault internal stub rejected the provided account API key"),
        "source error must be preserved"
    );
    assert_no_leaks(
        &text,
        "pass-remote register failure output",
        &[
            ("account API client id", "stub-client-id"),
            ("account API client secret", "stub-client-secret"),
            ("password-store remote URL", "example-owner/password-store"),
        ],
    );
    Ok(())
}

#[test]
/// `verify-yubikey --check vault` が個人 vault adapter 境界を使い、secret と URL を出力しないことを検証する。
fn verify_yubikey_vault_check_uses_personal_vault_internal_stub() -> TestResult<()> {
    let output = dotfiles_with_env(
        ["secrets", "verify-yubikey", "--check", "vault"],
        [
            yubikey_stub_env(),
            (
                "DOTFILES_SECRETS_VAULT_STUB_SPEC_JSON",
                format!(
                    r#"{{"secrets":{{"password-store-remote":"git@github.com:example-owner/password-store.git","gpg-secret-key-backup":{}}}}}"#,
                    serde_json::to_string(&gpg_backup_envelope_json())?
                ),
            ),
        ],
    )?;

    let text = combined_output(&output);
    assert!(
        output.status.success(),
        "verify-yubikey --check vault should pass through personal vault stub"
    );
    assert!(
        text.contains(r#""port":"bitwarden-vault""#),
        "vault stub should be exercised"
    );
    assert_no_leaks(
        &text,
        "verify-yubikey vault output",
        &[
            ("account API client id", "stub-client-id"),
            ("account API client secret", "stub-client-secret"),
            ("password-store remote URL", "example-owner/password-store"),
        ],
    );
    Ok(())
}

#[test]
/// `gpg-backup register` が既存 envelope を個人 vault adapter 境界で照合することを検証する。
fn gpg_backup_register_uses_personal_vault_internal_stub() -> TestResult<()> {
    let output = dotfiles_with_env(
        ["secrets", "gpg-backup", "register"],
        [
            yubikey_stub_env(),
            gpg_stub_env(),
            git_stub_env(),
            (
                "DOTFILES_SECRETS_VAULT_STUB_SPEC_JSON",
                format!(
                    r#"{{"secrets":{{"gpg-secret-key-backup":{}}}}}"#,
                    serde_json::to_string(&gpg_backup_envelope_json())?
                ),
            ),
        ],
    )?;

    let text = combined_output(&output);
    assert!(
        output.status.success(),
        "gpg-backup register should validate existing personal vault envelope"
    );
    assert!(
        text.contains(r#""port":"bitwarden-vault""#),
        "vault stub should be exercised"
    );
    assert_no_leaks(
        &text,
        "gpg-backup register output",
        &[
            ("account API client id", "stub-client-id"),
            ("account API client secret", "stub-client-secret"),
        ],
    );
    assert_no_primary_fingerprint_fixture_leaks(&text, "gpg-backup register output");
    Ok(())
}

#[test]
/// `gpg-backup register` の失敗時も error chain を保ち、secret と fingerprint fixture 値を出さないことを検証する。
fn gpg_backup_register_failure_preserves_chain_without_secret_values() -> TestResult<()> {
    let output = dotfiles_with_env(
        ["secrets", "gpg-backup", "register"],
        [
            yubikey_stub_env(),
            gpg_stub_env(),
            git_stub_env(),
            (
                "DOTFILES_SECRETS_VAULT_STUB_SPEC_JSON",
                r#"{"auth_fails":true}"#.to_owned(),
            ),
        ],
    )?;

    let text = combined_output(&output);
    assert!(
        !output.status.success(),
        "vault auth failure should fail gpg-backup register"
    );
    assert!(
        text.contains("caused by:"),
        "CLI must render the anyhow source chain"
    );
    assert!(
        text.contains("Bitwarden vault internal stub rejected the provided account API key"),
        "source error must be preserved"
    );
    assert_no_leaks(
        &text,
        "gpg-backup register failure output",
        &[
            ("account API client id", "stub-client-id"),
            ("account API client secret", "stub-client-secret"),
        ],
    );
    assert_no_primary_fingerprint_fixture_leaks(&text, "gpg-backup register failure output");
    Ok(())
}

#[test]
/// `gpg-backup register` は既存 envelope の検証専用であり、missing 時は作成せず停止することを固定する。
fn gpg_backup_register_missing_envelope_fails_without_secret_values() -> TestResult<()> {
    let output = dotfiles_with_env(
        ["secrets", "gpg-backup", "register"],
        [
            yubikey_stub_env(),
            gpg_stub_env(),
            git_stub_env(),
            (
                "DOTFILES_SECRETS_VAULT_STUB_SPEC_JSON",
                r#"{"secrets":{}}"#.to_owned(),
            ),
        ],
    )?;

    let text = combined_output(&output);
    assert!(
        !output.status.success(),
        "missing gpg backup envelope should fail"
    );
    assert!(
        text.contains("gpg-secret-key-backup is not registered"),
        "missing envelope failure must explain precondition without creating a backup"
    );
    assert_no_leaks(
        &text,
        "missing envelope output",
        &[
            ("account API client id", "stub-client-id"),
            ("account API client secret", "stub-client-secret"),
        ],
    );
    assert_no_primary_fingerprint_fixture_leaks(&text, "missing envelope output");
    Ok(())
}

#[test]
/// `restore-gpg` が個人 vault / YubiKey / GPG stub 境界を通り、secret と fingerprint を出力しないことを検証する。
fn restore_gpg_uses_internal_stubs_without_exposing_secret_values() -> TestResult<()> {
    let output = dotfiles_with_env(
        ["secrets", "restore-gpg"],
        [
            yubikey_stub_env(),
            gpg_stub_env(),
            (
                "DOTFILES_SECRETS_VAULT_STUB_SPEC_JSON",
                format!(
                    r#"{{"secrets":{{"gpg-secret-key-backup":{}}}}}"#,
                    serde_json::to_string(&restore_gpg_envelope_json())?
                ),
            ),
        ],
    )?;

    let text = combined_output(&output);
    assert!(
        output.status.success(),
        "restore-gpg should pass through internal stubs"
    );
    assert!(
        text.contains(r#""port":"bitwarden-vault""#)
            && text.contains(r#""port":"yubikey""#)
            && text.contains(r#""port":"gpg""#),
        "restore-gpg should exercise vault, YubiKey, and GPG stubs"
    );
    assert_no_leaks(
        &text,
        "restore-gpg output",
        &[
            ("account API client id", "stub-client-id"),
            ("account API client secret", "stub-client-secret"),
            ("master password", "stub-master-password"),
            (
                "decrypted backup fingerprint",
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            ),
        ],
    );
    assert_no_primary_fingerprint_fixture_leaks(&text, "restore-gpg output");
    Ok(())
}

/// `restore-gpg` は復号済み backup と envelope metadata の primary fingerprint 不一致で停止する。
#[test]
fn restore_gpg_rejects_mismatched_envelope_without_exposing_secret_values() -> TestResult<()> {
    let output = dotfiles_with_env(
        ["secrets", "restore-gpg"],
        [
            yubikey_stub_env(),
            gpg_stub_env(),
            (
                "DOTFILES_SECRETS_VAULT_STUB_SPEC_JSON",
                format!(
                    r#"{{"secrets":{{"gpg-secret-key-backup":{}}}}}"#,
                    serde_json::to_string(&restore_gpg_mismatched_envelope_json())?
                ),
            ),
        ],
    )?;

    let text = combined_output(&output);
    assert!(
        !output.status.success(),
        "restore-gpg should reject mismatched envelope metadata"
    );
    assert!(
        text.contains("decrypted gpg backup primary fingerprint does not match"),
        "restore-gpg mismatch failure must preserve the application error"
    );
    assert!(
        text.contains(r#""port":"bitwarden-vault""#) && text.contains(r#""port":"yubikey""#),
        "restore-gpg mismatch should exercise binary vault/YubiKey stub path before import"
    );
    assert_no_leaks(
        &text,
        "restore-gpg mismatch output",
        &[
            ("account API client id", "stub-client-id"),
            ("account API client secret", "stub-client-secret"),
            ("master password", "stub-master-password"),
        ],
    );
    assert_no_primary_fingerprint_fixture_leaks(&text, "restore-gpg mismatch output");
    assert_no_mismatched_fingerprint_fixture_leaks(&text, "restore-gpg mismatch output");
    Ok(())
}

#[test]
/// `restore-pass` が password-store remote を clone しても URL 実値を stub 観測へ出さないことを検証する。
fn restore_pass_redacts_git_remote_in_internal_stub_observation() -> TestResult<()> {
    let output = dotfiles_with_env(
        ["secrets", "restore-pass"],
        [
            yubikey_stub_env(),
            gpg_stub_env(),
            (
                "DOTFILES_SECRETS_VAULT_STUB_SPEC_JSON",
                r#"{"secrets":{"password-store-remote":"git@github.com:example-owner/password-store.git"}}"#.to_owned(),
            ),
            (
                "DOTFILES_SECRETS_GIT_STUB_SPEC_JSON",
                r#"{"store_exists":false}"#.to_owned(),
            ),
        ],
    )?;

    let text = combined_output(&output);
    assert!(
        output.status.success(),
        "restore-pass should pass through internal stubs"
    );
    assert!(
        text.contains(r#""port":"git""#) && text.contains(r#""cloned_remote_count":1"#),
        "git stub should observe clone count without URL"
    );
    assert_no_leaks(
        &text,
        "restore-pass output",
        &[
            ("account API client id", "stub-client-id"),
            ("account API client secret", "stub-client-secret"),
            ("master password", "stub-master-password"),
            ("password-store remote URL", "example-owner/password-store"),
        ],
    );
    Ok(())
}

/// `restore-pass` は vault 由来 remote が GitHub SSH clone URL でない場合に clone 前で停止する。
#[test]
fn restore_pass_rejects_invalid_remote_without_cloning_or_exposing_url() -> TestResult<()> {
    let output = dotfiles_with_env(
        ["secrets", "restore-pass"],
        [
            yubikey_stub_env(),
            gpg_stub_env(),
            (
                "DOTFILES_SECRETS_VAULT_STUB_SPEC_JSON",
                r#"{"secrets":{"password-store-remote":"https://github.com/example-owner/password-store"}}"#.to_owned(),
            ),
            (
                "DOTFILES_SECRETS_GIT_STUB_SPEC_JSON",
                r#"{"store_exists":false}"#.to_owned(),
            ),
        ],
    )?;

    let text = combined_output(&output);
    assert!(
        !output.status.success(),
        "restore-pass should reject invalid vault remote"
    );
    assert!(
        text.contains("password-store-remote must be a git@github.com SSH clone URL"),
        "restore-pass invalid remote failure must preserve the domain error"
    );
    assert!(
        !text.contains(r#""port":"git""#) && !text.contains(r#""cloned_remote_count":1"#),
        "restore-pass invalid remote must stop before clone"
    );
    assert_no_leaks(
        &text,
        "restore-pass invalid remote output",
        &[
            ("account API client id", "stub-client-id"),
            ("account API client secret", "stub-client-secret"),
            ("master password", "stub-master-password"),
            ("password-store remote URL", "example-owner/password-store"),
        ],
    );
    Ok(())
}

/// `enroll-primary` は binary 経路で input port 由来の 2 bootstrap secret だけを保存する。
#[test]
fn enroll_primary_stores_only_bootstrap_secrets_with_internal_stub() -> TestResult<()> {
    let output = dotfiles_with_env(
        ["secrets", "yubikey", "enroll-primary"],
        [(
            "DOTFILES_SECRETS_YUBIKEY_STUB_SPEC_JSON",
            r#"{"yubikeys":[{"serial":1000,"fixture":"fresh"}]}"#.to_owned(),
        )],
    )?;

    let text = combined_output(&output);
    assert!(
        output.status.success(),
        "enroll-primary should store bootstrap secrets through YubiKey stub"
    );
    assert!(
        text.contains(r#""bitwarden-client-id":"<redacted>""#)
            && text.contains(r#""bitwarden-client-secret":"<redacted>""#),
        "YubiKey observation should contain only redacted bootstrap secret names"
    );
    assert_no_leaks(
        &text,
        "enroll-primary output",
        &[
            ("account API client id", "stub-client-id"),
            ("account API client secret", "stub-client-secret"),
            ("master password", "stub-master-password"),
            ("removed token name", "bws-access-token"),
            ("vault remote item name", "password-store-remote"),
            ("vault backup item name", "gpg-secret-key-backup"),
        ],
    );
    Ok(())
}

/// `enroll-primary` は複数 YubiKey 接続時に secret 入力や保存へ進まず停止する。
#[test]
fn enroll_primary_rejects_multiple_yubikeys_before_secret_input() -> TestResult<()> {
    let output = dotfiles_with_env(
        ["secrets", "yubikey", "enroll-primary"],
        [(
            "DOTFILES_SECRETS_YUBIKEY_STUB_SPEC_JSON",
            r#"{"yubikeys":[{"serial":1000,"fixture":"fresh"},{"serial":1001,"fixture":"fresh"}]}"#
                .to_owned(),
        )],
    )?;

    let text = combined_output(&output);
    assert!(
        !output.status.success(),
        "enroll-primary should reject multiple YubiKeys"
    );
    assert!(
        text.contains("multiple YubiKeys detected") && text.contains("connect exactly one YubiKey"),
        "multiple-device failure must explain the operation constraint"
    );
    assert_no_leaks(
        &text,
        "multiple-device failure output",
        &[
            ("account API client id", "stub-client-id"),
            ("account API client secret", "stub-client-secret"),
            ("bootstrap client id name", "bitwarden-client-id"),
            ("bootstrap client secret name", "bitwarden-client-secret"),
        ],
    );
    Ok(())
}

#[test]
/// `enroll-spare` が input port 由来の 2 secret だけを YubiKey storage に保存することを検証する。
fn enroll_spare_stores_only_bootstrap_secrets_with_internal_stub() -> TestResult<()> {
    let output = dotfiles_with_env(
        ["secrets", "yubikey", "enroll-spare"],
        [(
            "DOTFILES_SECRETS_YUBIKEY_STUB_SPEC_JSON",
            r#"{"yubikeys":[{"serial":1001,"fixture":"fresh"}]}"#.to_owned(),
        )],
    )?;

    let text = combined_output(&output);
    assert!(
        output.status.success(),
        "enroll-spare should store bootstrap secrets through YubiKey stub"
    );
    assert!(
        text.contains(r#""bitwarden-client-id":"<redacted>""#)
            && text.contains(r#""bitwarden-client-secret":"<redacted>""#),
        "YubiKey observation should contain only redacted bootstrap secret names"
    );
    assert_no_leaks(
        &text,
        "enroll-spare output",
        &[
            ("account API client id", "stub-client-id"),
            ("account API client secret", "stub-client-secret"),
            ("master password", "stub-master-password"),
            ("removed token name", "bws-access-token"),
            ("vault remote item name", "password-store-remote"),
            ("vault backup item name", "gpg-secret-key-backup"),
        ],
    );
    Ok(())
}

fn yubikey_stub_env() -> (&'static str, String) {
    (
        "DOTFILES_SECRETS_YUBIKEY_STUB_SPEC_JSON",
        r#"{"yubikeys":[{"serial":1000,"fixture":"seeded","bitwarden-client-id":"stub-client-id","bitwarden-client-secret":"stub-client-secret"}]}"#.to_owned(),
    )
}

fn gpg_stub_env() -> (&'static str, String) {
    (
        "DOTFILES_SECRETS_GPG_STUB_SPEC_JSON",
        r#"{"keys":{"0123456789abcdef0123456789abcdef01234567":{"keygrip":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","ssh_public_key":"ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGdvdGNoYS1kdW1teS1wdWJsaWMta2V5LTAxMjM="}}}"#.to_owned(),
    )
}

fn git_stub_env() -> (&'static str, String) {
    ("DOTFILES_SECRETS_GIT_STUB_SPEC_JSON", "{}".to_owned())
}

fn gpg_backup_envelope_json() -> String {
    r#"{"version":1,"metadata":{"primary_fingerprint":"0123456789abcdef0123456789abcdef01234567","exported_at":"2026-01-01T00:00:00Z","dek_alg":"aes-256-gcm","recipient_kek_alg":"rsa-oaep-sha256"},"recipients":[{"piv_slot":"82","public_key_fingerprint":"000003e8000003e8000003e8000003e8000003e8000003e8000003e8000003e8","wrapped_dek":"AQ=="},{"piv_slot":"82","public_key_fingerprint":"000003e9000003e9000003e9000003e9000003e9000003e9000003e9000003e9","wrapped_dek":"Ag=="}],"ciphertext":{"nonce":"AAAAAAAAAAAAAAAA","body":"AQID","tag":"AAAAAAAAAAAAAAAAAAAAAA=="}}"#.to_owned()
}

fn restore_gpg_envelope_json() -> String {
    r#"{"version":1,"metadata":{"primary_fingerprint":"0123456789abcdef0123456789abcdef01234567","exported_at":"2026-01-01T00:00:00Z","dek_alg":"aes-256-gcm","recipient_kek_alg":"rsa-oaep-sha256"},"recipients":[{"piv_slot":"82","public_key_fingerprint":"000003e8000003e8000003e8000003e8000003e8000003e8000003e8000003e8","wrapped_dek":"AQ=="},{"piv_slot":"82","public_key_fingerprint":"000003e9000003e9000003e9000003e9000003e9000003e9000003e9000003e9","wrapped_dek":"Ag=="}],"ciphertext":{"nonce":"AAAAAAAAAAAAAAAA","body":"MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWYwMTIzNDU2Nw==","tag":"AAAAAAAAAAAAAAAAAAAAAA=="}}"#.to_owned()
}

fn restore_gpg_mismatched_envelope_json() -> String {
    restore_gpg_envelope_json().replace(
        "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWYwMTIzNDU2Nw==",
        "ZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZg==",
    )
}
