//! restore-gpg の sequence と port 境界を確立し、GPG import ステップの追加を妨げない骨格。

use crate::Result;
use crate::secrets::{
    domain::{
        piv::{SecretName, validate_piv_pin_len},
        storage::SecretStorageReadIntent,
        values::{BwsSecretName, RestoreGpgCommand},
    },
    ports::{self, BwsClientPort, SecretStoragePort},
};

/// BWS から `gpg-secret-key-backup` を取得し、GPG keyring に import する。
///
/// serial 未指定時は device port で自動選択し、YubiKey から bws-access-token を読み出した
/// 上で BWS fetch を実行する。GPG import ステップは未実装であり、BWS fetch 成功を
/// 設計境界の確認点とする。
pub(crate) fn run_restore_gpg_with<
    B: ports::DeviceSerialPort
        + ports::DevicePinPolicyPort
        + ports::PinInputPort
        + SecretStoragePort
        + BwsClientPort,
>(
    command: RestoreGpgCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = boundary.resolve_device_serial(command.serial)?;
    let pin = if boundary.device_requires_pin(serial)? {
        let pin = boundary.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };
    let token_storage = SecretName::BwsAccessToken.storage_spec(serial);
    let token_inspection = boundary.inspect_secret_storage_read(serial, &token_storage)?;
    let token_intent =
        SecretStorageReadIntent::from_inspection(token_storage, token_inspection)?;
    let token = boundary
        .load_secret(serial, &token_intent, pin.as_ref())
        .map_err(|error| token_intent.decode_error(error))?;
    token_intent.validate_loaded_secret(&token)?;
    let _gpg_key =
        boundary.fetch_bws_secret(&token, BwsSecretName::GpgSecretKeyBackup)?;
    anyhow::bail!("restore-gpg: GPG import step is not yet implemented")
}
