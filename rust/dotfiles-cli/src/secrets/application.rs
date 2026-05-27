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

#[cfg(all(test, feature = "secrets-internal-test-stub"))]
pub(crate) mod app_test_support {
    // app/usecase test double は production source tree へ定義せず、internal test 用 support から読む。
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/secrets_application/app_test_support.rs"
    ));
}

#[cfg(all(test, feature = "secrets-internal-test-stub"))]
mod tests {
    use crate::Result;
    use crate::secrets::application::app_test_support::AppMockBoundary;
    use crate::secrets::domain::piv::SecretName;
    use crate::secrets::domain::values::{
        EnrollPrimaryCommand, EnrollSpareCommand, PutCommand, RotateBwsTokenCommand, SetupCommand,
    };

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
        boundary.device_serial_available = false;
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
        boundary.device_serial_available = false;
        let err =
            super::run_setup_with::run_setup_with(SetupCommand { serial: None }, &mut boundary)
                .expect_err("setup unexpectedly succeeded");
        assert_eq!(err.to_string(), "pass --serial in non-interactive use");
        Ok(())
    }

    #[test]
    fn put_rejects_tty_stdin_before_device_open() -> Result<()> {
        let mut boundary = AppMockBoundary::new();
        boundary.device_serial = 10;
        boundary.streamed_secret_error = Some("--stdin requires pipe or redirect input");
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
    fn rotate_bws_token_rejects_noninteractive_without_serial() -> Result<()> {
        let mut boundary = AppMockBoundary::new();
        boundary.device_serial_available = false;
        let err = super::run_rotate_bws_token_with_prompt::run_rotate_bws_token_with_prompt(
            RotateBwsTokenCommand { serial: None },
            &mut boundary,
        )
        .expect_err("rotate-bws-token unexpectedly succeeded");
        assert_eq!(err.to_string(), "pass --serial in non-interactive use");
        Ok(())
    }

    #[test]
    fn enroll_primary_rejects_tty_stdin_json_before_device_open() -> Result<()> {
        let mut boundary = AppMockBoundary::new().expect_setup();
        boundary.device_serial = 10;
        boundary.stdin_json_error = Some("--stdin-json requires pipe or redirect input");
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
        boundary.spare_serial = 20;
        boundary.stdin_json_error = Some("--stdin-json requires pipe or redirect input");
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
        boundary.device_serial = 10;
        boundary.primary_requires_pin = true;
        boundary.pin_error = Some("pin verification failed");
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
        boundary.spare_serial = 20;
        boundary.spare_requires_pin = true;
        boundary.pin_error = Some("pin verification failed");
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
}
