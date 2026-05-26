use anyhow::bail;

use crate::Result;
use crate::secrets::{
    domain::{RotateBwsTokenCommand, SecretName},
    ports::{self},
};

const NONINTERACTIVE_SERIAL_ERROR: &str = "pass --serial in non-interactive use";

/// stdin 入力で BWS token を更新し、YubiKey 保存状態を再検証する。
pub(crate) fn run_rotate_bws_token_with_stdin<
    B: ports::SecretStorePort + ports::SecretInputPort + ports::StorageVerifyPort + ports::ReportPort,
>(
    command: RotateBwsTokenCommand,
    boundary: &mut B,
) -> Result<()> {
    let Some(serial) = command.serial else {
        bail!(NONINTERACTIVE_SERIAL_ERROR);
    };
    let token = boundary.read_stdin_secret()?;
    boundary.store_secret(serial, SecretName::BwsAccessToken, true, token.as_ref())?;
    match boundary.verify_local_storage(serial) {
        Ok(()) => boundary.report_local_storage_verified(serial),
        Err(err) => boundary.report_local_storage_failed(serial).and(Err(err)),
    }
}
