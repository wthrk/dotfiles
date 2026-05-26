use anyhow::bail;

use crate::Result;
use crate::secrets::{
    domain::VerifyYubikeyCommand,
    ports::{self},
};

/// 保存済み secret の存在と、要求された外部確認項目を検証する。
pub(crate) fn run_verify_yubikey_with<B: ports::StorageVerifyPort + ports::ReportPort>(
    command: VerifyYubikeyCommand,
    boundary: &mut B,
) -> Result<()> {
    let serial = command.required_serial()?;
    let requested = command.requested_external_checks()?;
    boundary.verify_local_storage(serial)?;
    if !requested.is_empty() {
        boundary.report_external_checks_unavailable(serial, requested.iter().copied())?;
        let requested_names = requested
            .iter()
            .map(|check| check.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        bail!("external checks are not implemented yet: {requested_names}");
    }

    boundary.report_local_storage_verified(serial)
}
