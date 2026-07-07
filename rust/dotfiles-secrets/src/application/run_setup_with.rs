//! setup の順序責務だけを保持し、device 選択と PIV 実行の変更理由を分離する。

use crate::Result;
use crate::{
    domain::{
        commands::SetupCommand,
        piv::validate_piv_pin_len,
        storage::{SecretStorageSetupIntent, SecretStorageSetupProbe},
    },
    ports,
};

/// 対象 serial の YubiKey storage layout を初期化する。
///
/// setup 可否判定は domain intent、PIV 操作詳細は adapter 側へ委譲し、application では順序制御だけを保持する。
pub(crate) fn run_setup_with<D, P, S>(
    command: SetupCommand,
    device: &mut D,
    pin_policy: &mut impl ports::DevicePinPolicyPort,
    pin_input: &P,
    storage: &mut S,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    P: ports::PinInputPort,
    S: ports::SecretStoragePort,
{
    let serial = device.resolve_device_serial(command.serial)?;
    let probe = SecretStorageSetupProbe::expected();
    let inspection = storage.inspect_secret_storage_setup(serial, &probe)?;
    let intent = SecretStorageSetupIntent::from_inspection(inspection)?;
    let pin = if pin_policy.device_requires_pin(serial)? {
        let pin = pin_input.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };
    storage.initialize_secret_storage(serial, intent.clone(), pin.as_ref())?;
    storage.finalize_secret_storage_setup(serial, intent)
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{
            commands::SetupCommand, piv::PivApplicationVersion,
            storage::SecretStorageSetupInspection,
        },
        ports,
    };

    use super::run_setup_with;

    fn clean_setup_inspection() -> SecretStorageSetupInspection {
        SecretStorageSetupInspection {
            key_exists: false,
            piv_version: PivApplicationVersion::minimum_for_secret_storage(),
            pin_retries: 1,
            manifest_bytes: None,
            occupied_object_ids: Vec::new(),
        }
    }

    #[test]
    fn setup_initializes_storage_after_serial_resolution() -> crate::Result<()> {
        let mut device = ports::MockDeviceSerialPort::new();
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        let pin_input = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        let mut sequence = mockall::Sequence::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|requested| Ok(requested.unwrap_or(2001)));
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(clean_setup_inspection()));
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(false));
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _, _| Ok(()));
        storage
            .expect_finalize_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));

        run_setup_with(
            SetupCommand { serial: Some(2001) },
            &mut device,
            &mut pin_policy,
            &pin_input,
            &mut storage,
        )
    }

    #[test]
    fn setup_reads_pin_when_required() -> crate::Result<()> {
        let mut device = ports::MockDeviceSerialPort::new();
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        let mut pin_input = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        let mut sequence = mockall::Sequence::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(clean_setup_inspection()));
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(true));
        pin_input
            .expect_read_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| {
                Ok(
                    crate::support::protection::ProtectedSecret::from_test_bytes(b"123456")
                        .expect("test pin"),
                )
            });
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|serial, _, pin| *serial == 2001 && pin.is_some())
            .returning(|_, _, _| Ok(()));
        storage
            .expect_finalize_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));

        run_setup_with(
            SetupCommand { serial: Some(2001) },
            &mut device,
            &mut pin_policy,
            &pin_input,
            &mut storage,
        )
    }
}
