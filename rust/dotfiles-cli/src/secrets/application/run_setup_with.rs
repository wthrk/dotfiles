//! setup の順序責務だけを保持し、device 選択と PIV 実行の変更理由を分離する。

use crate::Result;
use crate::secrets::{
    domain::{
        storage::{SecretStorageSetupIntent, SecretStorageSetupProbe},
        values::SetupCommand,
    },
    ports,
};

/// 対象 serial の YubiKey storage layout を初期化する。
///
/// setup 可否判定は domain intent、PIV 操作詳細は adapter 側へ委譲し、application では順序制御だけを保持する。
pub(crate) fn run_setup_with<B: ports::DeviceSerialPort + ports::SecretStoragePort>(
    command: SetupCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = boundary.resolve_device_serial(command.serial)?;
    let probe = SecretStorageSetupProbe::expected();
    let inspection = boundary.inspect_secret_storage_setup(serial, &probe)?;
    let intent = SecretStorageSetupIntent::from_inspection(inspection)?;
    boundary.initialize_secret_storage(serial, intent)
}

#[cfg(all(test, feature = "secrets-internal-test-stub"))]
mod tests {
    use crate::Result;
    use crate::secrets::{
        application::app_test_support::AppMockBoundary, domain::values::SetupCommand,
    };

    use super::run_setup_with;

    #[test]
    fn setup_initializes_storage_after_serial_resolution() -> Result<()> {
        let mut boundary = AppMockBoundary::new()
            .expect_setup()
            .expect_setup_initialize();
        run_setup_with(SetupCommand { serial: Some(2001) }, &mut boundary)
    }

    #[test]
    fn setup_stops_when_setup_inspection_fails() {
        let mut boundary = AppMockBoundary::new();
        boundary.mock.expect_event("setup");
        boundary.mock.set_setup_failure(true);
        let result = run_setup_with(SetupCommand { serial: Some(2001) }, &mut boundary);
        assert!(
            result.is_err(),
            "setup inspection failure must stop initialization"
        );
    }
}
