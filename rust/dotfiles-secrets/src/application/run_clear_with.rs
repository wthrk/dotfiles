//! 予約済み YubiKey storage の clear 順序を保持する。

use crate::{
    Result,
    domain::{commands::ClearCommand, storage::SecretStorageClearIntent},
    ports,
};

/// 明示確認済みの対象 YubiKey で予約済み領域だけを clear する。
pub(crate) fn run_clear_with<D, S>(
    command: ClearCommand,
    device_serial: &mut D,
    storage_port: &mut S,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    S: ports::SecretStoragePort,
{
    command.ensure_confirmed()?;
    let serial = device_serial.resolve_device_serial(command.serial)?;
    storage_port.clear_secret_storage(serial, SecretStorageClearIntent::expected())
}

#[cfg(test)]
mod tests {
    use crate::{domain::commands::ClearCommand, ports};

    use super::run_clear_with;

    #[test]
    fn clear_requires_confirmation_before_device_access() {
        let mut device = ports::MockDeviceSerialPort::new();
        device.expect_resolve_device_serial().never();
        let mut storage = ports::MockSecretStoragePort::new();
        storage.expect_clear_secret_storage().never();

        assert!(
            run_clear_with(
                ClearCommand {
                    serial: None,
                    confirmed: false
                },
                &mut device,
                &mut storage,
            )
            .is_err()
        );
    }

    #[test]
    fn clear_resolves_a_single_device_when_serial_is_omitted() -> crate::Result<()> {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .withf(|serial| serial.is_none())
            .returning(|_| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_clear_secret_storage()
            .withf(|serial, intent| *serial == 2001 && intent.object_ids.len() == 5)
            .returning(|_, _| Ok(()));

        run_clear_with(
            ClearCommand {
                serial: None,
                confirmed: true,
            },
            &mut device,
            &mut storage,
        )
    }
}
