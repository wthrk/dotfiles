//! get use case の orchestration。

use crate::Result;
use crate::secrets::{
    domain::{piv::validate_piv_pin_len, storage::SecretStorageReadIntent, values::GetCommand},
    ports,
};

/// 指定された secret を YubiKey storage から読み出し、出力 port へ受け渡す。
///
/// 読み出し経路の secret 値を application 層で加工せず、復号と出力方針は adapter 側の責務境界へ固定する。
pub(crate) fn run_get_with<B: ports::YubikeyPort + ports::SecretCliPort>(
    command: GetCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = boundary.resolve_device_serial(command.serial)?;
    let pin = if boundary.device_requires_pin(serial)? {
        let pin = boundary.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };
    let storage = command.storage_spec(serial);
    let inspection = boundary.inspect_secret_storage_read(serial, &storage)?;
    let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
    let secret = boundary
        .load_secret(serial, &intent, pin.as_ref())
        .map_err(|error| intent.decode_error(error))?;
    intent.validate_loaded_secret(&secret)?;
    boundary.write_secret(&secret)
}

#[cfg(all(test, feature = "secrets-internal-test-stub"))]
mod tests {
    use crate::Result;
    use crate::secrets::{
        application::app_test_support::AppMockBoundary,
        domain::{piv::SecretName, values::GetCommand},
    };

    use super::run_get_with;

    #[test]
    fn get_loads_secret_and_writes_output() -> Result<()> {
        let mut boundary = AppMockBoundary::new();
        boundary
            .mock
            .set_loaded_secret_value(SecretName::BwEmail, b"user@example.com");
        run_get_with(
            GetCommand {
                serial: Some(2001),
                name: SecretName::BwEmail,
            },
            &mut boundary,
        )?;
        assert_eq!(
            boundary.mock.output_secret_value(),
            Some(b"user@example.com".to_vec())
        );
        Ok(())
    }

    #[test]
    fn get_reads_pin_when_device_requires_it() -> Result<()> {
        let mut boundary = AppMockBoundary::new();
        boundary.mock.set_primary_requires_pin(true);
        boundary.mock.expect_event("pin");
        run_get_with(
            GetCommand {
                serial: Some(2001),
                name: SecretName::BwEmail,
            },
            &mut boundary,
        )
    }
}
