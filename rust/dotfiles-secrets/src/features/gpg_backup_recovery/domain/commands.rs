//! GPG backup/recovery feature が所有する application input values。

use super::gpg_backup::PrimaryFingerprint;
use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RestoreGpgCommand {
    pub(crate) serial: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExportSshPublicKeyCommand {
    pub(crate) primary_fingerprint: PrimaryFingerprint,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegisterGpgBackupCommand {
    pub(crate) primary_fingerprint: Option<PrimaryFingerprint>,
    pub(crate) serial: Option<u32>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AddGpgBackupSpareCommand {
    pub(crate) unwrap_serial: Option<u32>,
    pub(crate) spare_serial: Option<u32>,
    pub(crate) assume_overwrite: bool,
}
impl AddGpgBackupSpareCommand {
    pub(crate) fn ensure_requested_serials_distinct(&self) -> Result<()> {
        if self.unwrap_serial.is_some() && self.unwrap_serial == self.spare_serial {
            anyhow::bail!("unwrap YubiKey serial and spare YubiKey serial must be different");
        }
        Ok(())
    }
    pub(crate) fn ensure_distinct_resolved_serials(
        &self,
        unwrap_serial: u32,
        spare_serial: u32,
    ) -> Result<()> {
        if unwrap_serial == spare_serial {
            anyhow::bail!("unwrap YubiKey serial and spare YubiKey serial must be different");
        }
        Ok(())
    }
}
