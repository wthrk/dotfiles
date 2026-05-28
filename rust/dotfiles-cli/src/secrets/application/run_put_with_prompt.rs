//! put(prompt) の順序責務を保持し、入力手段と保存実装の変更理由を usecase から分離する。

use crate::Result;
use crate::secrets::{
    domain::{storage::SecretStorageWriteIntent, values::PutCommand},
    ports,
};

/// 対話入力で取得した secret を対象 serial の YubiKey storage へ保存する。
///
/// 入力モードの可視/不可視判定は `SecretName` の domain 規則で決め、端末 I/O 実装詳細は adapter へ委譲する。
pub(crate) fn run_put_with_prompt<
    B: ports::DeviceSerialPort + ports::SecretInputPort + ports::SecretStoragePort,
>(
    command: PutCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = boundary.resolve_device_serial(command.serial)?;
    let storage = command.storage_spec(serial);
    let inspection = boundary.inspect_secret_storage_write(serial, &storage)?;
    SecretStorageWriteIntent::ensure_put_preconditions(&storage, &inspection, command.force)?;
    let secret = command.name.read_interactive_secret_with(
        || boundary.read_bw_email_secret(),
        || boundary.read_bw_password_secret(),
        || boundary.read_bws_access_token_secret(),
    )?;
    let intent = SecretStorageWriteIntent::put(storage, inspection, command.force, secret.len())?;
    boundary.store_secret(serial, intent, &secret)
}

#[cfg(test)]
mod tests {
    use crate::Result;
    use crate::secrets::{
        application::app_test_support::AppMockBoundary,
        domain::{piv::SecretName, values::PutCommand},
    };

    use super::run_put_with_prompt;

    #[test]
    fn put_prompt_stores_requested_secret() -> Result<()> {
        let mut boundary = AppMockBoundary::new();
        run_put_with_prompt(
            PutCommand {
                serial: Some(2001),
                name: SecretName::BwEmail,
                force: false,
            },
            &mut boundary,
        )?;
        assert_eq!(boundary.mock.stores(), vec![SecretName::BwEmail]);
        Ok(())
    }

    #[test]
    fn put_prompt_stops_when_secret_read_fails() {
        let mut boundary = AppMockBoundary::new();
        boundary
            .mock
            .set_secret_error(SecretName::BwEmail, "read failed");
        let result = run_put_with_prompt(
            PutCommand {
                serial: Some(2001),
                name: SecretName::BwEmail,
                force: false,
            },
            &mut boundary,
        );
        assert!(result.is_err(), "secret read failure must stop put flow");
    }

    #[test]
    fn put_prompt_checks_storage_before_reading_secret() {
        let mut boundary = AppMockBoundary::new();
        boundary.mock.set_write_object_exists(true);
        boundary.mock.set_secret_error(
            SecretName::BwEmail,
            "secret should not be read before preflight",
        );

        let result = run_put_with_prompt(
            PutCommand {
                serial: Some(2001),
                name: SecretName::BwEmail,
                force: false,
            },
            &mut boundary,
        );

        assert!(
            result.is_err(),
            "occupied storage should stop before prompt read"
        );
    }
}
