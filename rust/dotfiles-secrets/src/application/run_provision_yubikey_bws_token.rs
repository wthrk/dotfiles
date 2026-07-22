//! provisioning source の BWS token 保存を、単一 PIV 管理 session に閉じる。
//!
//! shell の process 境界をまたぐ `status → clear/setup → put` は使わない。PIV PIN を一回だけ
//! 取得して高水準 session を開始した後、観測から local decrypt 検証まで同じ storage port に適用する。

use crate::{
    Result,
    domain::{
        commands::ProvisionBwsTokenCommand,
        storage::{
            SecretStorageClearIntent, SecretStorageReadIntent, SecretStorageSetupIntent,
            SecretStorageSetupProbe, SecretStorageStatus, SecretStorageVerificationPlan,
            SecretStorageWriteIntent, is_observed_storage_invalid,
        },
    },
    ports,
};

/// BWS access token を必要時だけ保存し、同一 PIV handle で復号可能なことまで確認する。
///
/// serial を確定してから PIN を一回だけ読み、`begin_piv_management_session` に同じ serial と PIN を
/// 渡す。正常な既存 token は再入力・書込みをせず local decrypt 検証だけを行う。観測済みの typed
/// storage 不整合だけを clear し、transport / discovery / SDK error を不整合へ推測して mutation しない。
/// opt-in debug の presentation はこの use case 外の composition decorator が通常の port 呼び出しを
/// 観測して担い、use case は表示用 DTO や technical failure 分類を持たない。
pub(crate) fn run_provision_yubikey_bws_token(
    command: ProvisionBwsTokenCommand,
    device: &mut dyn ports::DeviceSerialPort,
    piv_pin: &dyn ports::PivPinInputPort,
    secret_input: &dyn ports::BitwardenClientSecretInputPort,
    storage: &mut dyn ports::SecretStoragePort,
) -> Result<()> {
    let serial = device.resolve_device_serial(command.serial)?;
    let pin = piv_pin.read_piv_pin_secret()?;
    storage.begin_piv_management_session(serial, pin)?;

    let storage_spec = command.storage_spec(serial);
    let status_inspection = storage.inspect_secret_storage_status(serial, &storage_spec)?;
    let status = SecretStorageStatus::from_inspections([(storage_spec.clone(), status_inspection)]);
    match status {
        Ok(status) if status.stored().contains(&storage_spec.name) => {
            return verify_local_storage(serial, storage);
        }
        Ok(_) => ensure_initialized_storage(serial, storage)?,
        Err(error) if is_observed_storage_invalid(&error) => {
            let clear = SecretStorageClearIntent::expected();
            let public_key_spki = storage.clear_secret_storage(serial, clear.clone())?;
            storage.finalize_secret_storage_setup(
                serial,
                clear.manifest_for_generated_public_key(public_key_spki)?,
            )?;
        }
        Err(error) => return Err(error),
    }

    let inspection = storage.inspect_secret_storage_write(serial, &storage_spec)?;
    // post-clear/setup state は token を読む前に検査し、不正なら token を消費せず停止する。
    let _preflight =
        SecretStorageWriteIntent::preflight_put(storage_spec.clone(), &inspection, false)?;
    let token = secret_input.read_bitwarden_client_secret()?;
    let intent = SecretStorageWriteIntent::put(storage_spec, inspection, false, token.len())?;
    storage.store_secret(serial, intent, &token)?;
    verify_local_storage(serial, storage)
}

/// 完全に空の予約領域だけを初期化し、manifest を確定する。
///
/// 呼び出し側は PIV management session を開始済みでなければならない。setup inspection が既存状態を
/// 許容しない場合はこの helper 内で停止し、key generation や manifest 書込みへ進ませない。
fn ensure_initialized_storage(
    serial: u32,
    storage: &mut dyn ports::SecretStoragePort,
) -> Result<()> {
    let inspection =
        storage.inspect_secret_storage_setup(serial, &SecretStorageSetupProbe::expected())?;
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

/// 保存済み token が同一 PIV session で復号・検証できることを確認する。
///
/// inspection、domain intent、load、value validation のいずれかで失敗した時点で caller へ返し、
/// token の再入力や storage mutation を行わない。
fn verify_local_storage(serial: u32, storage: &mut dyn ports::SecretStoragePort) -> Result<()> {
    for storage_spec in SecretStorageVerificationPlan::for_serial(serial).into_targets() {
        let inspection = storage.inspect_secret_storage_read(serial, &storage_spec)?;
        let intent = SecretStorageReadIntent::from_inspection(storage_spec, inspection)?;
        let secret = storage
            .load_secret(serial, &intent)
            .map_err(|error| intent.decode_error(error))?;
        intent.validate_loaded_secret(&secret)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! `provision-bws-token` の fail-closed lifecycle を port mock で確認する。

    use crate::{
        domain::{
            commands::ProvisionBwsTokenCommand,
            manifest::SecretManifest,
            storage::{SecretStorageReadInspection, SecretStorageStatusInspection},
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_provision_yubikey_bws_token;

    #[test]
    fn existing_token_is_verified_without_token_input_or_mutation() -> crate::Result<()> {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut pin = ports::io::MockPivPinInputPort::new();
        pin.expect_read_piv_pin_secret()
            .returning(|| ProtectedSecret::from_test_bytes(b"123456"));
        let mut input = ports::MockBitwardenClientSecretInputPort::new();
        input.expect_read_bitwarden_client_secret().never();
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_begin_piv_management_session()
            .returning(|_, _| Ok(()));
        storage
            .expect_inspect_secret_storage_status()
            .returning(|_, _| {
                Ok(SecretStorageStatusInspection {
                    manifest_bytes: Some(SecretManifest::fixture_v2().encode().expect("manifest")),
                    object_present: true,
                    object_exists: true,
                })
            });
        storage
            .expect_inspect_secret_storage_read()
            .returning(|_, _| {
                Ok(SecretStorageReadInspection {
                    manifest_bytes: Some(SecretManifest::fixture_v2().encode().expect("manifest")),
                    encoded: Some(vec![1]),
                })
            });
        storage
            .expect_load_secret()
            .returning(|_, _| ProtectedSecret::from_test_bytes(b"token"));
        storage.expect_store_secret().never();
        storage.expect_clear_secret_storage().never();

        run_provision_yubikey_bws_token(
            ProvisionBwsTokenCommand { serial: Some(2001) },
            &mut device,
            &pin,
            &input,
            &mut storage,
        )?;
        Ok(())
    }
}
