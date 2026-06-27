//! setup の順序責務だけを保持し、device 選択と PIV 実行の変更理由を分離する。

use crate::Result;
use crate::secrets::{
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
    D: ports::yubikey::DeviceSerialPort,
    S: ports::yubikey::SecretStoragePort,
{
    let _ = command;
    let serial = device.resolve_device_serial()?;
    let probe = SecretStorageSetupProbe::expected();
    let inspection = storage.inspect_secret_storage_setup(serial, &probe)?;
    let intent = SecretStorageSetupIntent::from_inspection(inspection)?;
    storage.initialize_secret_storage(serial, intent.clone(), None)?;
    storage.finalize_secret_storage_setup(serial, intent)
}

/// setup use case が device 解決、domain intent、storage 初期化の順序だけを担うことを検証する。
#[cfg(test)]
mod tests {
    use crate::secrets::{
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

    /// serial 解決後に setup inspection から intent を作り、initialize/finalize を順に呼ぶ。
    #[test]
    fn setup_initializes_storage_after_serial_resolution() -> crate::Result<()> {
        let mut device = ports::yubikey::MockDeviceSerialPort::new();
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        let mut sequence = mockall::Sequence::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(2001));
        storage
            .expect_inspect_secret_storage_setup()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(clean_setup_inspection()));
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

        run_setup_with(SetupCommand, &mut device, &mut storage)
    }
}
