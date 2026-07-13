//! status の順序責務だけを保持し、secret 本文の復号・出力経路を作らない。

use crate::{
    Result,
    domain::{commands::StatusCommand, piv::SecretStorageSpec, storage::SecretStorageStatus},
    ports,
};

/// 指定された YubiKey に設定済みの bootstrap secret 名だけを出力する。
///
/// すべての予約 object を inspect してから manifest と予約 slot を domain で検証し、正常な manifest
/// の場合は保存済み object の名前だけを報告する。inspection と名前の報告だけを port 経由で行い、
/// touch、secret 本文の復号はこの use case の責務に含めない。
pub(crate) fn run_status_with<D, S, O>(
    command: StatusCommand,
    device_serial: &mut D,
    storage_port: &mut S,
    output: &O,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    S: ports::SecretStoragePort,
    O: ports::SecretStorageStatusOutputPort,
{
    let serial = device_serial.resolve_device_serial(command.serial)?;
    let inspections = SecretStorageSpec::all_for_serial(serial).map(|storage| {
        let inspection = storage_port.inspect_secret_storage_write(serial, &storage)?;
        Ok((storage, inspection))
    });
    let status = SecretStorageStatus::from_inspections(
        inspections.into_iter().collect::<Result<Vec<_>>>()?,
    )?;
    output.write_secret_storage_status(&status)
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{
            commands::StatusCommand, manifest::SecretManifest, piv::SecretName,
            storage::SecretStorageWriteInspection,
        },
        ports,
    };

    use super::run_status_with;

    fn inspection(object_exists: bool) -> crate::Result<SecretStorageWriteInspection> {
        Ok(SecretStorageWriteInspection {
            manifest_bytes: Some(SecretManifest::expected().encode()?),
            object_exists,
            reserved_slot_key_exists: true,
            reserved_slot_certificate_exists: false,
        })
    }

    #[test]
    fn status_outputs_only_stored_secret_names() -> crate::Result<()> {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(4)
            .returning(|_, _| inspection(true));
        let mut output = ports::MockSecretStorageStatusOutputPort::new();
        output
            .expect_write_secret_storage_status()
            .withf(|status| {
                status.stored()
                    == [
                        SecretName::BwEmail,
                        SecretName::BwPassword,
                        SecretName::BitwardenClientId,
                        SecretName::BitwardenClientSecret,
                    ]
            })
            .returning(|_| Ok(()));

        run_status_with(
            StatusCommand { serial: Some(2001) },
            &mut device,
            &mut storage,
            &output,
        )
    }

    #[test]
    fn status_reports_the_present_name_for_a_manifest_with_a_missing_reserved_object()
    -> crate::Result<()> {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(4)
            .returning(|_, spec| inspection(spec.name != SecretName::BitwardenClientSecret));
        let mut output = ports::MockSecretStorageStatusOutputPort::new();
        output
            .expect_write_secret_storage_status()
            .withf(|status| {
                status.stored()
                    == [
                        SecretName::BwEmail,
                        SecretName::BwPassword,
                        SecretName::BitwardenClientId,
                    ]
            })
            .returning(|_| Ok(()));

        run_status_with(
            StatusCommand { serial: Some(2001) },
            &mut device,
            &mut storage,
            &output,
        )
    }

    #[test]
    fn status_accepts_an_empty_reserved_storage_without_output() -> crate::Result<()> {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(4)
            .returning(|_, _| {
                Ok(SecretStorageWriteInspection {
                    manifest_bytes: None,
                    object_exists: false,
                    reserved_slot_key_exists: false,
                    reserved_slot_certificate_exists: false,
                })
            });
        let mut output = ports::MockSecretStorageStatusOutputPort::new();
        output
            .expect_write_secret_storage_status()
            .withf(|status| status.stored().is_empty())
            .returning(|_| Ok(()));

        run_status_with(
            StatusCommand { serial: Some(2001) },
            &mut device,
            &mut storage,
            &output,
        )
    }

    #[test]
    fn status_rejects_a_manifestless_reserved_object() {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(4)
            .returning(|_, _| {
                Ok(SecretStorageWriteInspection {
                    manifest_bytes: None,
                    object_exists: true,
                    reserved_slot_key_exists: true,
                    reserved_slot_certificate_exists: false,
                })
            });
        let mut output = ports::MockSecretStorageStatusOutputPort::new();
        output.expect_write_secret_storage_status().never();

        assert!(
            run_status_with(
                StatusCommand { serial: Some(2001) },
                &mut device,
                &mut storage,
                &output,
            )
            .is_err()
        );
    }

    #[test]
    fn status_stops_without_output_when_manifest_is_missing() {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(4)
            .returning(|_, _| {
                Ok(SecretStorageWriteInspection {
                    manifest_bytes: None,
                    object_exists: true,
                    reserved_slot_key_exists: true,
                    reserved_slot_certificate_exists: false,
                })
            });
        let mut output = ports::MockSecretStorageStatusOutputPort::new();
        output.expect_write_secret_storage_status().never();

        let result = run_status_with(
            StatusCommand { serial: Some(2001) },
            &mut device,
            &mut storage,
            &output,
        );

        assert!(result.is_err());
    }

    #[test]
    fn status_stops_without_output_when_manifest_is_invalid() {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_write()
            .times(4)
            .returning(|_, _| {
                Ok(SecretStorageWriteInspection {
                    manifest_bytes: Some(b"invalid manifest".to_vec()),
                    object_exists: true,
                    reserved_slot_key_exists: true,
                    reserved_slot_certificate_exists: false,
                })
            });
        let mut output = ports::MockSecretStorageStatusOutputPort::new();
        output.expect_write_secret_storage_status().never();

        let result = run_status_with(
            StatusCommand { serial: Some(2001) },
            &mut device,
            &mut storage,
            &output,
        );

        assert!(result.is_err());
    }
}
