//! put stdin use case の orchestration。

use crate::Result;
use crate::secrets::{
    domain::{storage::SecretStorageWriteIntent, values::PutCommand},
    ports,
};

/// 非対話 stdin から受け取った secret を対象 serial の YubiKey storage へ保存する。
///
/// use case は入力取得と保存順序のみを担い、stdin 条件やサイズ制約は adapter 実装側へ閉じ込める。
pub(crate) fn run_put_with_stdin<B: ports::YubikeyPort + ports::SecretCliPort>(
    command: PutCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = command.required_serial()?;
    let secret = boundary.read_streamed_secret()?;
    let storage = command.storage_spec(serial);
    let inspection = boundary.inspect_secret_storage_write(serial, &storage)?;
    let intent = SecretStorageWriteIntent::put(storage, inspection, command.force, secret.len())?;
    boundary.store_secret(serial, intent, &secret)
}

#[cfg(all(test, feature = "secrets-internal-test-stub"))]
mod tests {
    use crate::Result;
    use crate::secrets::{
        application::app_test_support::AppMockBoundary,
        domain::{piv::SecretName, values::PutCommand},
    };

    use super::run_put_with_stdin;

    #[test]
    fn put_stdin_stores_requested_secret() -> Result<()> {
        let mut boundary = AppMockBoundary::new();
        run_put_with_stdin(
            PutCommand {
                serial: Some(2001),
                name: SecretName::BwsAccessToken,
                force: false,
            },
            &mut boundary,
        )?;
        assert_eq!(boundary.mock.stores(), vec![SecretName::BwsAccessToken]);
        Ok(())
    }

    #[test]
    fn put_stdin_requires_serial() {
        let mut boundary = AppMockBoundary::new();
        let result = run_put_with_stdin(
            PutCommand {
                serial: None,
                name: SecretName::BwsAccessToken,
                force: false,
            },
            &mut boundary,
        );
        assert!(result.is_err(), "stdin path requires explicit serial");
    }
}
