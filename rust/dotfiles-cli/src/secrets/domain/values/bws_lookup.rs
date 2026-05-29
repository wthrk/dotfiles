use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BwsSecretName {
    GpgSecretKeyBackup,
    PasswordStoreRemote,
}

impl BwsSecretName {
    pub fn key(self) -> &'static str {
        match self {
            Self::GpgSecretKeyBackup => "gpg-secret-key-backup",
            Self::PasswordStoreRemote => "password-store-remote",
        }
    }

    pub fn resolve_id<I: Clone>(
        self,
        candidates: impl IntoIterator<Item = BwsLookupCandidate<I>>,
        project_id: impl std::fmt::Display,
    ) -> Result<I> {
        resolve_single_bws_lookup(
            candidates,
            self.key(),
            || {
                format!(
                    "bws secret key not found in project {project_id}: {}",
                    self.key()
                )
            },
            || {
                format!(
                    "multiple bws secret keys matched in project {project_id}: {}",
                    self.key()
                )
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwsProjectId(String);

impl BwsProjectId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for BwsProjectId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwsSecretId(String);

impl BwsSecretId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BwsProjectName;

impl BwsProjectName {
    pub const DOTFILES_SECRET_RECOVERY: Self = Self;

    pub fn as_str(self) -> &'static str {
        "dotfiles-secret-recovery"
    }

    pub fn resolve_id<I: Clone>(
        self,
        candidates: impl IntoIterator<Item = BwsLookupCandidate<I>>,
    ) -> Result<I> {
        resolve_single_bws_lookup(
            candidates,
            self.as_str(),
            || format!("bws project not found: {}", self.as_str()),
            || format!("multiple bws projects matched: {}", self.as_str()),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwsLookupCandidate<I> {
    pub id: I,
    pub name: String,
}

fn resolve_single_bws_lookup<I: Clone>(
    candidates: impl IntoIterator<Item = BwsLookupCandidate<I>>,
    expected_name: &str,
    missing: impl FnOnce() -> String,
    ambiguous: impl FnOnce() -> String,
) -> Result<I> {
    let mut matches = candidates
        .into_iter()
        .filter(|candidate| candidate.name == expected_name);
    let Some(candidate) = matches.next() else {
        return Err(invalid_input(missing()).into());
    };
    if matches.next().is_some() {
        return Err(invalid_input(ambiguous()).into());
    }
    Ok(candidate.id.clone())
}

fn invalid_input(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod bws_lookup_tests {
    use super::{BwsLookupCandidate, BwsProjectName, BwsSecretName};

    fn candidate(id: &'static str, name: &'static str) -> BwsLookupCandidate<&'static str> {
        BwsLookupCandidate {
            id,
            name: name.to_owned(),
        }
    }

    #[test]
    fn bws_project_name_resolves_unique_id() {
        let result = BwsProjectName::DOTFILES_SECRET_RECOVERY.resolve_id([
            candidate("other", "other-project"),
            candidate("target", "dotfiles-secret-recovery"),
        ]);

        assert_eq!(result.ok(), Some("target"));
    }

    #[test]
    fn bws_project_name_rejects_missing_candidate() {
        let result = BwsProjectName::DOTFILES_SECRET_RECOVERY
            .resolve_id([candidate("other", "other-project")]);

        match result {
            Ok(value) => panic!("unexpected project id: {value}"),
            Err(error) => assert_eq!(
                error.to_string(),
                "bws project not found: dotfiles-secret-recovery"
            ),
        }
    }

    #[test]
    fn bws_project_name_rejects_duplicate_candidates() {
        let result = BwsProjectName::DOTFILES_SECRET_RECOVERY.resolve_id([
            candidate("first", "dotfiles-secret-recovery"),
            candidate("second", "dotfiles-secret-recovery"),
        ]);

        match result {
            Ok(value) => panic!("unexpected project id: {value}"),
            Err(error) => assert_eq!(
                error.to_string(),
                "multiple bws projects matched: dotfiles-secret-recovery"
            ),
        }
    }

    #[test]
    fn bws_secret_name_resolves_unique_id() {
        let result = BwsSecretName::GpgSecretKeyBackup.resolve_id(
            [
                candidate("target", "gpg-secret-key-backup"),
                candidate("other", "password-store-remote"),
            ],
            "project-1",
        );

        assert_eq!(result.ok(), Some("target"));
    }

    #[test]
    fn bws_secret_name_rejects_missing_candidate() {
        let result = BwsSecretName::PasswordStoreRemote
            .resolve_id([candidate("other", "gpg-secret-key-backup")], "project-1");

        match result {
            Ok(value) => panic!("unexpected secret id: {value}"),
            Err(error) => assert_eq!(
                error.to_string(),
                "bws secret key not found in project project-1: password-store-remote"
            ),
        }
    }

    #[test]
    fn bws_secret_name_rejects_duplicate_candidates() {
        let result = BwsSecretName::GpgSecretKeyBackup.resolve_id(
            [
                candidate("first", "gpg-secret-key-backup"),
                candidate("second", "gpg-secret-key-backup"),
            ],
            "project-1",
        );

        match result {
            Ok(value) => panic!("unexpected secret id: {value}"),
            Err(error) => assert_eq!(
                error.to_string(),
                "multiple bws secret keys matched in project project-1: gpg-secret-key-backup"
            ),
        }
    }
}
