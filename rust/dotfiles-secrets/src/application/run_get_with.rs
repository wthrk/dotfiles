//! get の順序責務だけを保持し、secret 復号・出力の実装詳細を port 境界の外へ固定する。

use crate::Result;
use crate::{
    domain::{commands::GetCommand, piv::validate_piv_pin_len, storage::SecretStorageReadIntent},
    ports,
};

/// 指定された secret を YubiKey storage から読み出し、出力 port へ受け渡す。
///
/// 読み出し経路の secret 値を application 層で加工せず、復号と出力方針は adapter 側の責務境界へ固定する。
pub(crate) fn run_get_with<D, P, S, O>(
    command: GetCommand,
    device_serial: &mut D,
    pin_policy: &mut impl ports::DevicePinPolicyPort,
    process: &P,
    storage_port: &mut S,
    output: &O,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    P: ports::PinInputPort,
    S: ports::SecretStoragePort,
    O: ports::SecretOutputPort,
{
    let serial = device_serial.resolve_device_serial(command.serial)?;
    let pin = if pin_policy.device_requires_pin(serial)? {
        let pin = process.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };
    let storage = command.storage_spec(serial);
    let inspection = storage_port.inspect_secret_storage_read(serial, &storage)?;
    let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
    let secret = storage_port
        .load_secret(serial, &intent, pin.as_ref())
        .map_err(|error| intent.decode_error(error))?;
    intent.validate_loaded_secret(&secret)?;
    output.write_secret(&secret)
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{
            commands::GetCommand, manifest::SecretManifest, piv::SecretName,
            storage::SecretStorageReadInspection,
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_get_with;

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn read_inspection() -> SecretStorageReadInspection {
        SecretStorageReadInspection {
            manifest_bytes: Some(SecretManifest::expected().encode().expect("manifest")),
            encoded: Some(vec![1]),
        }
    }

    #[test]
    fn get_loads_secret_and_writes_output() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|requested| Ok(requested.expect("serial")));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(false));
        let process = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|serial, storage| *serial == 2001 && storage.name == SecretName::BitwardenClientSecret)
            .returning(|_, _| Ok(read_inspection()));
        storage
            .expect_load_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _, _| Ok(material(b"token")));
        let mut output = ports::MockSecretOutputPort::new();
        output
            .expect_write_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|secret| secret.len() == b"token".len())
            .returning(|_| Ok(()));

        run_get_with(
            GetCommand {
                name: SecretName::BitwardenClientSecret,
                serial: Some(2001),
            },
            &mut device_serial,
            &mut pin_policy,
            &process,
            &mut storage,
            &output,
        )
    }

    #[test]
    fn get_reads_pin_only_when_device_requires_it() -> crate::Result<()> {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .returning(|_| Ok(2001));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(true));
        let mut process = ports::MockPinInputPort::new();
        process
            .expect_read_pin()
            .times(1)
            .returning(|| Ok(material(b"123456")));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .returning(|_, _| Ok(read_inspection()));
        storage
            .expect_load_secret()
            .times(1)
            .withf(|_, _, pin| pin.is_some())
            .returning(|_, _, _| Ok(material(b"token")));
        let mut output = ports::MockSecretOutputPort::new();
        output.expect_write_secret().times(1).returning(|_| Ok(()));

        run_get_with(
            GetCommand {
                name: SecretName::BitwardenClientSecret,
                serial: Some(2001),
            },
            &mut device_serial,
            &mut pin_policy,
            &process,
            &mut storage,
            &output,
        )
    }
}
