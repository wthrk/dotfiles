//! setup の順序責務だけを保持し、device 選択と PIV 実行の変更理由を分離する。

use crate::Result;
use crate::{
    domain::{
        commands::SetupCommand,
        storage::{SecretStorageSetupIntent, SecretStorageSetupProbe},
    },
    ports,
};

/// 対象 serial の YubiKey storage layout を初期化する。
///
/// setup 可否判定は domain intent、PIV 操作詳細は adapter 側へ委譲し、application では順序制御だけを保持する。
pub(crate) fn run_setup_with<D, S>(
    command: SetupCommand,
    device: &mut D,
    storage: &mut S,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    S: ports::SecretStoragePort,
{
    let serial = device.resolve_device_serial(command.serial)?;
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
    };

    use super::run_setup_with;

    fn clean_setup_inspection() -> SecretStorageSetupInspection {
        SecretStorageSetupInspection {
            key_exists: false,
            piv_version: PivApplicationVersion::minimum_for_secret_storage(),
            manifest_bytes: None,
            occupied_object_ids: Vec::new(),
        }
    }

    #[test]
    fn setup_initializes_storage_without_any_pin_capability() -> crate::Result<()> {
        // PIN の入力・検証・要求は run_setup_with の契約に存在しない。
        let mut device = ports::MockDeviceSerialPort::new();
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
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| {
                Ok(SecretManifest::fixture_v2()
                    .slot_public_key_spki
                    .expect("fixture SPKI"))
            });
        storage
            .expect_finalize_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));

        run_setup_with(
            SetupCommand { serial: Some(2001) },
            &mut device,
            &mut storage,
        )
    }

    #[test]
    fn setup_is_a_noop_for_normal_empty_v2_storage() -> crate::Result<()> {
        let mut device = ports::MockDeviceSerialPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|requested| Ok(requested.unwrap_or(2001)));
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .returning(|_, _| {
                Ok(SecretStorageSetupInspection {
                    key_exists: true,
                    piv_version: PivApplicationVersion::minimum_for_secret_storage(),
                    manifest_bytes: Some(SecretManifest::fixture_v2().encode()?),
                    occupied_object_ids: Vec::new(),
                })
            });
        storage.expect_initialize_secret_storage().times(0);
        storage.expect_finalize_secret_storage_setup().times(0);

        run_setup_with(
            SetupCommand { serial: Some(2001) },
            &mut device,
            &mut storage,
        )
    }

    #[test]
    fn setup_migrates_v1_only_after_public_key_metadata_is_available() -> crate::Result<()> {
        let mut device = ports::MockDeviceSerialPort::new();
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
            .returning(|_, _| {
                Ok(SecretStorageSetupInspection {
                    key_exists: true,
                    piv_version: PivApplicationVersion::minimum_for_secret_storage(),
                    manifest_bytes: Some(
                        SecretManifest {
                            version: 1,
                            app: crate::domain::manifest::MANIFEST_APP.to_owned(),
                            slot_public_key_spki: None,
                        }
                        .encode()?,
                    ),
                    occupied_object_ids: Vec::new(),
                })
            });
        storage
            .expect_initialize_secret_storage()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| {
                Ok(SecretManifest::fixture_v2()
                    .slot_public_key_spki
                    .expect("fixture SPKI"))
            });
        storage
            .expect_finalize_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));

        run_setup_with(
            SetupCommand { serial: Some(2001) },
            &mut device,
            &mut storage,
        )
    }
}
