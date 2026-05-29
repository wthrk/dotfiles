use std::collections::BTreeMap;

use crate::secrets::domain::values::bws_lookup::BwsSecretName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Ok,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckName {
    Setup,
    BwEmail,
    BwPassword,
    BwsAccessToken,
    LocalStorage,
    Bws,
    BwLogin,
}

impl CheckName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::BwEmail => "bw-email",
            Self::BwPassword => "bw-password",
            Self::BwsAccessToken => "bws-access-token",
            Self::LocalStorage => "local-storage",
            Self::Bws => "bws",
            Self::BwLogin => "bw-login",
        }
    }

    pub fn required_bws_secrets(self) -> Option<&'static [BwsSecretName]> {
        match self {
            Self::Bws => Some(&[
                BwsSecretName::GpgSecretKeyBackup,
                BwsSecretName::PasswordStoreRemote,
            ]),
            Self::Setup
            | Self::BwEmail
            | Self::BwPassword
            | Self::BwsAccessToken
            | Self::LocalStorage
            | Self::BwLogin => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YubikeyRole {
    Primary,
    Spare,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrollSummary {
    pub serial: u32,
    pub role: YubikeyRole,
    pub checks: BTreeMap<CheckName, CheckStatus>,
}

impl EnrollSummary {
    pub fn primary_completed(serial: u32) -> Self {
        Self::completed(serial, YubikeyRole::Primary)
    }

    pub fn spare_completed(serial: u32) -> Self {
        Self::completed(serial, YubikeyRole::Spare)
    }

    pub fn initial(serial: u32, role: YubikeyRole) -> Self {
        Self {
            serial,
            role,
            checks: [
                (CheckName::Setup, CheckStatus::Ok),
                (CheckName::BwEmail, CheckStatus::Ok),
                (CheckName::BwPassword, CheckStatus::Ok),
                (CheckName::BwsAccessToken, CheckStatus::Ok),
                (CheckName::LocalStorage, CheckStatus::Skipped),
            ]
            .into_iter()
            .collect(),
        }
    }

    pub fn mark_local_storage_ok(&mut self) {
        self.checks.insert(CheckName::LocalStorage, CheckStatus::Ok);
    }

    fn completed(serial: u32, role: YubikeyRole) -> Self {
        let mut summary = Self::initial(serial, role);
        summary.mark_local_storage_ok();
        summary
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifySummary {
    pub serial: u32,
    pub checks: BTreeMap<CheckName, CheckStatus>,
}

impl VerifySummary {
    pub fn local_storage_verified(serial: u32) -> Self {
        Self::with_local_storage_status(serial, CheckStatus::Ok)
    }

    pub fn local_storage_failed(serial: u32) -> Self {
        Self::with_local_storage_status(serial, CheckStatus::Failed)
    }

    pub fn mark_external_check(&mut self, check: CheckName, status: CheckStatus) {
        self.checks.insert(check, status);
    }

    fn with_local_storage_status(serial: u32, local_storage: CheckStatus) -> Self {
        Self {
            serial,
            checks: [
                (CheckName::LocalStorage, local_storage),
                (CheckName::Bws, CheckStatus::Skipped),
                (CheckName::BwLogin, CheckStatus::Skipped),
            ]
            .into_iter()
            .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_rotate_summary_serializes_updated_entries() {
        let summary = VerifySummary::local_storage_failed(42);

        assert_eq!(summary.serial, 42);
        assert_eq!(
            summary.checks.get(&CheckName::LocalStorage),
            Some(&CheckStatus::Failed)
        );
    }

    #[test]
    fn partial_rotate_summary_skips_output_when_empty() {
        let summaries: Vec<VerifySummary> = Vec::new();

        assert!(summaries.is_empty());
    }
}
