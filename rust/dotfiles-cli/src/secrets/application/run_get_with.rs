//! get の順序責務だけを保持し、secret 復号・出力の実装詳細を port 境界の外へ固定する。

use crate::Result;
use crate::secrets::{
    domain::{commands::GetCommand, piv::validate_piv_pin_len, storage::SecretStorageReadIntent},
    ports,
};

/// 指定された secret を YubiKey storage から読み出し、出力 port へ受け渡す。
///
/// 読み出し経路の secret 値を application 層で加工せず、復号と出力方針は adapter 側の責務境界へ固定する。
pub(crate) fn run_get_with<P, S, O>(
    command: GetCommand,
    device: &mut impl ports::yubikey::YubiKeyDevicePort,
    process: &P,
    storage_port: &mut S,
    output: &O,
) -> Result<()>
where
    P: ports::io::PinInputPort,
    S: ports::yubikey::SecretStoragePort,
    O: ports::io::SecretOutputPort,
{
    let serial = device.resolve_device_serial()?;
    let pin = if device.device_requires_pin(serial)? {
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

/// get use case が復号結果を application で加工せず output port へ渡す順序を検証する。
#[cfg(test)]
mod tests {
    use crate::secrets::{
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

    /// 指定 secret の inspection/load 後に output port へ保護値を渡す。
    #[test]
    fn get_loads_secret_and_writes_output() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::yubikey::MockYubiKeyDevicePort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(2001));
        device
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(false));
        let process = ports::io::MockPinInputPort::new();
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|serial, storage| {
                *serial == 2001 && storage.name == SecretName::BitwardenClientSecret
            })
            .returning(|_, _| Ok(read_inspection()));
        storage
            .expect_load_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _, _| Ok(material(b"token")));
        let mut output = ports::io::MockSecretOutputPort::new();
        output
            .expect_write_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|secret| secret.len() == b"token".len())
            .returning(|_| Ok(()));

        run_get_with(
            GetCommand {
                name: SecretName::BitwardenClientSecret,
            },
            &mut device,
            &process,
            &mut storage,
            &output,
        )
    }

    /// PIN は device policy が要求した場合だけ読み、load 境界へ渡す。
    #[test]
    fn get_reads_pin_only_when_device_requires_it() -> crate::Result<()> {
        let mut device = ports::yubikey::MockYubiKeyDevicePort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2001));
        device
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(true));
        let mut process = ports::io::MockPinInputPort::new();
        process
            .expect_read_pin()
            .times(1)
            .returning(|| Ok(material(b"123456")));
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .returning(|_, _| Ok(read_inspection()));
        storage
            .expect_load_secret()
            .times(1)
            .withf(|_, _, pin| pin.is_some())
            .returning(|_, _, _| Ok(material(b"token")));
        let mut output = ports::io::MockSecretOutputPort::new();
        output.expect_write_secret().times(1).returning(|_| Ok(()));

        run_get_with(
            GetCommand {
                name: SecretName::BitwardenClientSecret,
            },
            &mut device,
            &process,
            &mut storage,
            &output,
        )
    }

    /// inspection 失敗時は load/output へ進まず、error をそのまま伝播して停止する。
    #[test]
    fn get_stops_when_inspection_fails() {
        let mut device = ports::yubikey::MockYubiKeyDevicePort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2001));
        device
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(false));
        let process = ports::io::MockPinInputPort::new();
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .returning(|_, _| Err(anyhow::anyhow!("inspect read failed")));
        storage.expect_load_secret().times(0);
        let mut output = ports::io::MockSecretOutputPort::new();
        output.expect_write_secret().times(0);

        let result = run_get_with(
            GetCommand {
                name: SecretName::BitwardenClientSecret,
            },
            &mut device,
            &process,
            &mut storage,
            &output,
        );

        let error = result.expect_err("inspection failure must stop before output");
        assert!(
            format!("{error:#}").contains("inspect read failed"),
            "inspection failure must propagate without reaching the output port"
        );
    }

    /// load_secret 失敗は intent.decode_error で対象 secret の domain error へ写像し、source を保つ。
    #[test]
    fn get_maps_load_failure_to_decode_error_without_output() {
        let mut device = ports::yubikey::MockYubiKeyDevicePort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2001));
        device
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(false));
        let process = ports::io::MockPinInputPort::new();
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .returning(|_, _| Ok(read_inspection()));
        storage
            .expect_load_secret()
            .times(1)
            .returning(|_, _, _| Err(anyhow::anyhow!("ciphertext rejected")));
        let mut output = ports::io::MockSecretOutputPort::new();
        output.expect_write_secret().times(0);

        let result = run_get_with(
            GetCommand {
                name: SecretName::BitwardenClientSecret,
            },
            &mut device,
            &process,
            &mut storage,
            &output,
        );

        let error = result.expect_err("load failure must stop before output");
        let rendered = format!("{error:#}");
        assert!(
            rendered.contains("failed to decode bitwarden-client-secret"),
            "load failure must be mapped to the target secret decode error"
        );
        assert!(
            rendered.contains("ciphertext rejected"),
            "decode error mapping must preserve the underlying source chain"
        );
    }

    /// 復号済み secret が値制約を満たさない場合は output port へ渡さず停止する。
    #[test]
    fn get_stops_when_loaded_secret_is_invalid() {
        let mut device = ports::yubikey::MockYubiKeyDevicePort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2001));
        device
            .expect_device_requires_pin()
            .times(1)
            .returning(|_| Ok(false));
        let process = ports::io::MockPinInputPort::new();
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .returning(|_, _| Ok(read_inspection()));
        storage
            .expect_load_secret()
            .times(1)
            .returning(|_, _, _| Ok(material(b"")));
        let mut output = ports::io::MockSecretOutputPort::new();
        output.expect_write_secret().times(0);

        let result = run_get_with(
            GetCommand {
                name: SecretName::BitwardenClientSecret,
            },
            &mut device,
            &process,
            &mut storage,
            &output,
        );

        let error = result.expect_err("invalid loaded secret must stop before output");
        assert!(
            format!("{error:#}").contains("must not be empty"),
            "invalid loaded secret must fail validation before reaching the output port"
        );
    }
}
