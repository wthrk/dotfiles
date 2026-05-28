//! `dotfiles secrets` の application 層。
//!
//! 個別 use case の orchestration を提供し、command 選択は entrypoint 側が担う。

pub(crate) mod run_enroll_primary_with_prompt;
pub(crate) mod run_enroll_primary_with_stdin_json;
pub(crate) mod run_enroll_spare_with_prompt;
pub(crate) mod run_enroll_spare_with_stdin_json;
pub(crate) mod run_get_with;
pub(crate) mod run_put_with_prompt;
pub(crate) mod run_put_with_stdin;
pub(crate) mod run_rotate_bws_token_with_prompt;
pub(crate) mod run_rotate_bws_token_with_stdin;
pub(crate) mod run_setup_with;
pub(crate) mod run_verify_yubikey_with;

#[cfg(test)]
pub(crate) mod app_test_support {
    // Rust private module の usecase を検査する test-only bridge。
    //
    // mockall 共通 support の本体は `tests/secrets_application/` に置き、production build には
    // 含めない。bridge は port trait 契約で usecase を駆動し、runtime real/stub 分岐や
    // production command path の変更を作らない。
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/secrets_application/app_test_support.rs"
    ));
}

#[cfg(test)]
mod tests {
    use crate::Result;
    use crate::secrets::application::app_test_support::AppMockBoundary;
    use crate::secrets::domain::piv::SecretName;
    use crate::secrets::domain::values::{
        EnrollPrimaryCommand, EnrollSpareCommand, GetCommand, PutCommand, RotateBwsTokenCommand,
        SetupCommand,
    };
    use sha2::{Digest, Sha256};

    fn assert_secret_bytes_eq(actual: Option<&[u8]>, expected: &[u8], label: &str) {
        let actual = actual.unwrap_or_else(|| panic!("{label} secret is missing"));
        let actual_digest: [u8; 32] = Sha256::digest(actual).into();
        let expected_digest: [u8; 32] = Sha256::digest(expected).into();

        assert_eq!(actual.len(), expected.len(), "{label} length mismatch");
        assert_eq!(actual_digest, expected_digest, "{label} digest mismatch");
    }

    #[test]
    fn enroll_spare_rejects_same_primary_and_spare_serial() -> Result<()> {
        let mut boundary = AppMockBoundary::new();
        let command = EnrollSpareCommand {
            primary_serial: Some(10),
            spare_serial: Some(10),
        };
        let err = super::run_enroll_spare_with_prompt::run_enroll_spare_with_prompt(
            command,
            &mut boundary,
        )
        .expect_err("enroll-spare accepted duplicate serials");
        assert_eq!(
            err.to_string(),
            "primary and spare YubiKey serial must be different"
        );
        Ok(())
    }

    #[test]
    fn put_rejects_noninteractive_without_serial_before_device_open() -> Result<()> {
        let mut boundary = AppMockBoundary::new();
        boundary.mock.set_primary_available(false);
        let command = PutCommand {
            name: SecretName::BwsAccessToken,
            serial: None,
            force: false,
        };
        let err = super::run_put_with_stdin::run_put_with_stdin(command, &mut boundary)
            .expect_err("put unexpectedly succeeded");
        assert_eq!(err.to_string(), "pass --serial in non-interactive use");
        Ok(())
    }

    #[test]
    fn setup_rejects_noninteractive_without_serial_before_device_open() -> Result<()> {
        let mut boundary = AppMockBoundary::new();
        boundary.mock.set_primary_available(false);
        let err =
            super::run_setup_with::run_setup_with(SetupCommand { serial: None }, &mut boundary)
                .expect_err("setup unexpectedly succeeded");
        assert_eq!(err.to_string(), "pass --serial in non-interactive use");
        Ok(())
    }

    #[test]
    fn put_rejects_tty_stdin_before_device_open() -> Result<()> {
        let mut boundary = AppMockBoundary::new();
        boundary.mock.set_primary_serial(10);
        boundary
            .mock
            .set_streamed_secret_error("--stdin requires pipe or redirect input");
        let command = PutCommand {
            name: SecretName::BwsAccessToken,
            serial: Some(10),
            force: false,
        };
        let err = super::run_put_with_stdin::run_put_with_stdin(command, &mut boundary)
            .expect_err("put unexpectedly accepted tty stdin");
        assert_eq!(err.to_string(), "--stdin requires pipe or redirect input");
        Ok(())
    }

    #[test]
    fn put_rejects_noninteractive_without_stdin_option() -> Result<()> {
        let mut boundary = AppMockBoundary::new();
        boundary.mock.set_primary_serial(10);
        boundary.mock.set_secret_error(
            SecretName::BwsAccessToken,
            "pass --stdin in non-interactive use",
        );
        let command = PutCommand {
            name: SecretName::BwsAccessToken,
            serial: Some(10),
            force: true,
        };
        let err = super::run_put_with_prompt::run_put_with_prompt(command, &mut boundary)
            .expect_err("put unexpectedly accepted missing --stdin");
        assert_eq!(err.to_string(), "pass --stdin in non-interactive use");
        Ok(())
    }

    #[test]
    fn rotate_bws_token_rejects_noninteractive_without_serial() -> Result<()> {
        let mut boundary = AppMockBoundary::new();
        boundary.mock.set_primary_available(false);
        let err = super::run_rotate_bws_token_with_prompt::run_rotate_bws_token_with_prompt(
            RotateBwsTokenCommand { serial: None },
            &mut boundary,
        )
        .expect_err("rotate-bws-token unexpectedly succeeded");
        assert_eq!(err.to_string(), "pass --serial in non-interactive use");
        Ok(())
    }

    #[test]
    fn rotate_bws_token_rejects_already_updated_serial() -> Result<()> {
        let mut boundary = AppMockBoundary::new();
        boundary
            .mock
            .set_store_already_updated_failure(SecretName::BwsAccessToken);
        let err = super::run_rotate_bws_token_with_prompt::run_rotate_bws_token_with_prompt(
            RotateBwsTokenCommand { serial: Some(2001) },
            &mut boundary,
        )
        .expect_err("rotate-bws-token accepted duplicate serial update");
        assert_eq!(err.to_string(), "selected YubiKey was already updated");
        Ok(())
    }

    #[test]
    fn enroll_primary_rejects_tty_stdin_json_before_device_open() -> Result<()> {
        let mut boundary = AppMockBoundary::new().expect_setup();
        boundary.mock.set_primary_serial(10);
        boundary
            .mock
            .set_stdin_json_error("--stdin-json requires pipe or redirect input");
        let err = super::run_enroll_primary_with_stdin_json::run_enroll_primary_with_stdin_json(
            EnrollPrimaryCommand { serial: Some(10) },
            &mut boundary,
        )
        .expect_err("enroll-primary unexpectedly accepted tty stdin-json");
        assert_eq!(
            err.to_string(),
            "--stdin-json requires pipe or redirect input"
        );
        Ok(())
    }

    #[test]
    fn enroll_spare_rejects_tty_stdin_json_before_device_open() -> Result<()> {
        let mut boundary = AppMockBoundary::new().expect_setup();
        boundary.mock.set_spare_serial(20);
        boundary
            .mock
            .set_stdin_json_error("--stdin-json requires pipe or redirect input");
        let command = EnrollSpareCommand {
            primary_serial: Some(10),
            spare_serial: Some(20),
        };
        let err = super::run_enroll_spare_with_stdin_json::run_enroll_spare_with_stdin_json(
            command,
            &mut boundary,
        )
        .expect_err("enroll-spare unexpectedly accepted tty stdin-json");
        assert_eq!(
            err.to_string(),
            "--stdin-json requires pipe or redirect input"
        );
        Ok(())
    }

    #[test]
    fn enroll_primary_stdin_json_stops_before_secret_read_when_pin_verification_fails() -> Result<()>
    {
        let mut boundary = AppMockBoundary::new().expect_setup().expect_store_times(3);
        boundary.mock.set_primary_serial(10);
        boundary.mock.set_primary_requires_pin(true);
        boundary.mock.set_pin_error("pin verification failed");
        let err = super::run_enroll_primary_with_stdin_json::run_enroll_primary_with_stdin_json(
            EnrollPrimaryCommand { serial: Some(10) },
            &mut boundary,
        )
        .expect_err("enroll-primary unexpectedly succeeded");
        assert_eq!(err.to_string(), "pin verification failed");
        Ok(())
    }

    #[test]
    fn enroll_spare_stdin_json_stops_before_secret_read_when_pin_verification_fails() -> Result<()>
    {
        let mut boundary = AppMockBoundary::new().expect_setup().expect_store_times(3);
        boundary.mock.set_spare_serial(20);
        boundary.mock.set_spare_requires_pin(true);
        boundary.mock.set_pin_error("pin verification failed");
        let command = EnrollSpareCommand {
            primary_serial: Some(10),
            spare_serial: Some(20),
        };
        let err = super::run_enroll_spare_with_stdin_json::run_enroll_spare_with_stdin_json(
            command,
            &mut boundary,
        )
        .expect_err("enroll-spare unexpectedly succeeded");
        assert_eq!(err.to_string(), "pin verification failed");
        Ok(())
    }

    #[test]
    fn enroll_primary_rejects_empty_secret_before_setup() -> Result<()> {
        let mut boundary = AppMockBoundary::new();
        boundary.mock.expect_event("setup");
        boundary.mock.set_primary_serial(10);
        boundary.mock.set_secret_value(SecretName::BwEmail, b"");
        let err = super::run_enroll_primary_with_prompt::run_enroll_primary_with_prompt(
            EnrollPrimaryCommand { serial: Some(10) },
            &mut boundary,
        )
        .expect_err("enroll-primary accepted empty bw-email");

        assert_eq!(err.to_string(), "bw-email must not be empty");
        assert!(boundary.mock.stores().is_empty());
        Ok(())
    }

    #[test]
    fn enroll_spare_rejects_empty_secret_before_setup() -> Result<()> {
        let mut boundary = AppMockBoundary::new();
        boundary.mock.expect_event_times("setup", 0);
        boundary.mock.set_primary_serial(10);
        boundary.mock.set_spare_serial(20);
        boundary
            .mock
            .set_loaded_secret_value(SecretName::BwEmail, b"");
        let command = EnrollSpareCommand {
            primary_serial: Some(10),
            spare_serial: Some(20),
        };
        let err = super::run_enroll_spare_with_prompt::run_enroll_spare_with_prompt(
            command,
            &mut boundary,
        )
        .expect_err("enroll-spare accepted empty bw-email");

        assert_eq!(err.to_string(), "bw-email must not be empty");
        assert!(boundary.mock.stores().is_empty());
        Ok(())
    }

    #[test]
    fn setup_stops_when_management_auth_precondition_fails() -> Result<()> {
        let mut boundary = AppMockBoundary::new();
        boundary.mock.set_primary_serial(10);
        boundary.mock.set_setup_failure(true);
        boundary.mock.expect_event_times("setup-initialize", 0);

        let err =
            super::run_setup_with::run_setup_with(SetupCommand { serial: Some(10) }, &mut boundary)
                .expect_err("setup unexpectedly ignored precondition failure");

        assert_eq!(err.to_string(), "mockall app failed: storage setup inspect");
        Ok(())
    }

    #[test]
    fn setup_uses_management_auth_for_precondition_and_manifest_write() -> Result<()> {
        let mut boundary = AppMockBoundary::new()
            .expect_setup()
            .expect_setup_initialize();
        boundary.mock.set_primary_serial(10);

        super::run_setup_with::run_setup_with(SetupCommand { serial: Some(10) }, &mut boundary)
    }

    #[test]
    fn put_get_and_verify_round_trip_through_device() -> Result<()> {
        let mut boundary = AppMockBoundary::new().expect_store_times(3);
        boundary.mock.set_primary_serial(10);
        boundary
            .mock
            .set_secret_value(SecretName::BwEmail, b"user@example.com");
        boundary
            .mock
            .set_secret_value(SecretName::BwPassword, b"password");
        boundary
            .mock
            .set_secret_value(SecretName::BwsAccessToken, b"token");

        for name in SecretName::iter() {
            super::run_put_with_prompt::run_put_with_prompt(
                PutCommand {
                    name,
                    serial: Some(10),
                    force: false,
                },
                &mut boundary,
            )?;
        }
        super::run_get_with::run_get_with(
            GetCommand {
                name: SecretName::BwEmail,
                serial: Some(10),
            },
            &mut boundary,
        )?;

        assert_secret_bytes_eq(
            boundary.mock.output_secret_value().as_deref(),
            b"user@example.com",
            "bw-email output",
        );
        for name in SecretName::iter() {
            assert!(
                boundary
                    .mock
                    .stored_secret_value(name)
                    .is_some_and(|secret| !secret.is_empty()),
                "{name} should be stored"
            );
        }
        Ok(())
    }

    #[test]
    fn put_uses_management_auth_for_each_secret_write() -> Result<()> {
        let mut boundary = AppMockBoundary::new().expect_store_times(2);
        boundary.mock.set_primary_serial(10);

        for name in [SecretName::BwEmail, SecretName::BwPassword] {
            super::run_put_with_prompt::run_put_with_prompt(
                PutCommand {
                    name,
                    serial: Some(10),
                    force: false,
                },
                &mut boundary,
            )?;
        }
        Ok(())
    }

    #[test]
    fn rotate_bws_token_preserves_other_secrets() -> Result<()> {
        let mut boundary = AppMockBoundary::new().expect_store_times(1).expect_report();
        boundary.mock.set_primary_serial(10);
        boundary
            .mock
            .set_loaded_secret_value(SecretName::BwEmail, b"user@example.com");
        boundary
            .mock
            .set_loaded_secret_value(SecretName::BwPassword, b"password");
        boundary
            .mock
            .set_loaded_secret_value(SecretName::BwsAccessToken, b"old-token");
        boundary
            .mock
            .set_secret_value(SecretName::BwsAccessToken, b"new-token");

        super::run_rotate_bws_token_with_prompt::run_rotate_bws_token_with_prompt(
            RotateBwsTokenCommand { serial: Some(10) },
            &mut boundary,
        )?;

        assert_secret_bytes_eq(
            boundary
                .mock
                .stored_secret_value(SecretName::BwEmail)
                .as_deref(),
            b"user@example.com",
            "bw-email stored",
        );
        assert_secret_bytes_eq(
            boundary
                .mock
                .stored_secret_value(SecretName::BwPassword)
                .as_deref(),
            b"password",
            "bw-password stored",
        );
        assert_secret_bytes_eq(
            boundary
                .mock
                .stored_secret_value(SecretName::BwsAccessToken)
                .as_deref(),
            b"new-token",
            "bws-access-token stored",
        );
        Ok(())
    }

    #[test]
    fn rotate_uses_management_auth_for_token_replacement() -> Result<()> {
        let mut boundary = AppMockBoundary::new().expect_store_times(1).expect_report();
        boundary.mock.set_primary_serial(10);

        super::run_rotate_bws_token_with_prompt::run_rotate_bws_token_with_prompt(
            RotateBwsTokenCommand { serial: Some(10) },
            &mut boundary,
        )
    }
}
