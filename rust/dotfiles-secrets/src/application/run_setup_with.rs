//! setup の PIV management lifecycle を保持する。

use crate::{
    Result,
    domain::{
        commands::SetupCommand,
        storage::{SecretStorageSetupIntent, SecretStorageSetupProbe},
    },
    ports,
};

/// 管理 PIN と同一 PIV session を開始してから storage layout を初期化する。
pub(crate) fn run_setup(
    command: SetupCommand,
    device: &mut dyn ports::DeviceSerialPort,
    piv_pin: &dyn ports::PivPinInputPort,
    storage: &mut dyn ports::SecretStoragePort,
) -> Result<()> {
    let serial = device.resolve_device_serial(command.serial)?;
    let pin = piv_pin.read_piv_pin_secret()?;
    storage.begin_piv_management_session(serial, pin)?;
    let probe = SecretStorageSetupProbe::expected();
    let inspection = storage.inspect_secret_storage_setup(serial, &probe)?;
    let intent = SecretStorageSetupIntent::from_inspection(inspection)?;
    if !intent.requires_public_key_spki() {
        return Ok(());
    }
    let public_key_spki = storage.initialize_secret_storage(serial, intent.clone())?;
    if intent.requires_finalization() {
        storage.finalize_secret_storage_setup(
            serial,
            intent.manifest_for_public_key(public_key_spki)?,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{
            commands::SetupCommand, manifest::SecretManifest, piv::PivApplicationVersion,
            storage::SecretStorageSetupInspection,
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_setup;

    #[test]
    fn setup_runner_starts_one_session_before_initialization() -> crate::Result<()> {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut pin = ports::io::MockPivPinInputPort::new();
        pin.expect_read_piv_pin_secret()
            .returning(|| ProtectedSecret::from_test_bytes(b"123456"));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_management_session()
            .times(1)
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_setup()
            .returning(|_, _| {
                Ok(SecretStorageSetupInspection {
                    key_exists: false,
                    piv_version: PivApplicationVersion::minimum_for_secret_storage(),
                    manifest_bytes: None,
                    occupied_object_ids: Vec::new(),
                })
            });
        storage
            .expect_initialize_secret_storage()
            .returning(|_, _| {
                Ok(SecretManifest::fixture_v2()
                    .slot_public_key_spki
                    .expect("fixture SPKI"))
            });
        storage
            .expect_finalize_secret_storage_setup()
            .returning(|_, _| Ok(()));

        run_setup(
            SetupCommand { serial: Some(2001) },
            &mut device,
            &pin,
            &mut storage,
        )
    }
}
