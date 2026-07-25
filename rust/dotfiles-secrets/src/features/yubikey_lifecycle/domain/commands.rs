//! YubiKey lifecycle feature が所有する application input values。

use anyhow::Result;

use super::piv::{SecretName, SecretStorageSpec};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SetupCommand {
    pub(crate) serial: Option<u32>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PutCommand {
    pub(crate) name: SecretName,
    pub(crate) serial: Option<u32>,
    pub(crate) force: bool,
}
impl PutCommand {
    pub(crate) fn from_cli_name(name: &str, serial: Option<u32>, force: bool) -> Result<Self> {
        let name = name
            .parse()
            .map_err(|error: String| anyhow::anyhow!("{error}"))?;
        Ok(Self {
            name,
            serial,
            force,
        })
    }

    pub(crate) fn storage_spec(&self, serial: u32) -> SecretStorageSpec {
        self.name.storage_spec(serial)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StatusCommand {
    pub(crate) serial: Option<u32>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClearCommand {
    pub(crate) serial: Option<u32>,
    pub(crate) confirmed: bool,
}
impl ClearCommand {
    pub(crate) fn ensure_confirmed(self) -> Result<()> {
        self.confirmed.then_some(()).ok_or_else(|| {
            anyhow::anyhow!("refusing to clear YubiKey secret storage without --yes")
        })
    }
}
