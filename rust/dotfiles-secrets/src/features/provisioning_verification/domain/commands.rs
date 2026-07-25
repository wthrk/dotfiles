//! Provisioning/verification feature が所有する application input values。

use crate::features::{
    provisioning_verification::domain::verification::{CheckName, ExternalCheck},
    yubikey_lifecycle::ports::public::{SecretName, SecretStorageSpec},
};
use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EnrollPrimaryCommand {
    pub(crate) serial: Option<u32>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EnrollSpareCommand {
    pub(crate) primary_serial: Option<u32>,
    pub(crate) spare_serial: Option<u32>,
}
impl EnrollSpareCommand {
    pub(crate) fn ensure_requested_serials_distinct(&self) -> Result<()> {
        if self.primary_serial.is_some() && self.primary_serial == self.spare_serial {
            anyhow::bail!("primary and spare YubiKey serial must be different");
        }
        Ok(())
    }
    pub(crate) fn ensure_requested_primary_differs_from_spare(
        &self,
        spare_serial: u32,
    ) -> Result<()> {
        if self.primary_serial == Some(spare_serial) {
            anyhow::bail!("primary and spare YubiKey serial must be different");
        }
        Ok(())
    }
    pub(crate) fn ensure_distinct_resolved_serials(
        &self,
        primary_serial: u32,
        spare_serial: u32,
    ) -> Result<()> {
        if primary_serial == spare_serial {
            anyhow::bail!("primary and spare YubiKey serial must be different");
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RotateBwsTokenCommand {
    pub(crate) serial: Option<u32>,
}
impl RotateBwsTokenCommand {
    pub(crate) fn target_secret(self) -> SecretName {
        SecretName::BitwardenClientSecret
    }
    pub(crate) fn storage_spec(self, serial: u32) -> SecretStorageSpec {
        self.target_secret().storage_spec(serial)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProvisionBwsTokenCommand {
    pub(crate) serial: Option<u32>,
}
impl ProvisionBwsTokenCommand {
    pub(crate) fn storage_spec(self, serial: u32) -> SecretStorageSpec {
        SecretName::BitwardenClientSecret.storage_spec(serial)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifyYubikeyCommand {
    pub(crate) serial: Option<u32>,
    pub(crate) checks: Vec<ExternalCheck>,
    pub(crate) all: bool,
}
impl VerifyYubikeyCommand {
    pub(crate) fn requested_external_checks(&self) -> Result<Vec<CheckName>> {
        if self.all && !self.checks.is_empty() {
            anyhow::bail!("--all and --check cannot be used together");
        }
        if self.all {
            return Ok(vec![CheckName::Bws]);
        }
        let mut checks = Vec::new();
        for check in &self.checks {
            let name = match check {
                ExternalCheck::Bws => CheckName::Bws,
            };
            if !checks.contains(&name) {
                checks.push(name);
            }
        }
        Ok(checks)
    }
}
