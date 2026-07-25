//! Bitwarden Secrets Manager の lookup 対象同一性と opaque ID を表す domain model。
//!
//! 固定 project / secret name、一意解決、0 件/複数件の failure 化は SDK 実装詳細ではなく
//! 復旧対象の業務規則であるため、この module に閉じる。

use anyhow::Result;

/// Bitwarden Secrets Manager から取得する secret 名の閉じた集合。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BwsSecretName {
    GpgSecretKeyBackup,
    PasswordStoreRemote,
}

impl BwsSecretName {
    /// Bitwarden Secrets Manager 上で exact match する固定 secret key を返す。
    ///
    /// この key は外部 SDK の都合ではなく復旧設計上の対象同一性であり、adapter はこの
    /// domain rule を BWS API の検索条件へ翻訳する責務だけを持つ。
    pub fn key(self) -> &'static str {
        match self {
            Self::GpgSecretKeyBackup => "gpg-secret-key-backup",
            Self::PasswordStoreRemote => "password-store-remote",
        }
    }

    /// BWS secret 候補が、この復旧対象の固定 secret key に一致するかを判定する。
    ///
    /// exact match の対象同一性は domain rule であり、application は候補数の分岐だけを扱う。
    pub fn matches_candidate<I>(self, candidate: &BwsLookupCandidate<I>) -> bool {
        candidate.name == self.key()
    }

    /// BWS secret 候補から、この復旧対象に一致する ID を一意に解決する。
    ///
    /// 0 件と複数件はどちらも domain failure であり、adapter はこの判定を再実装しない。
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

/// Bitwarden Secrets Manager project ID を domain lookup の opaque 値として保持する。
///
/// SDK 型へは依存せず、同一性確認と後続 port 呼び出しの境界値としてだけ使う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwsProjectId(String);

impl BwsProjectId {
    /// adapter が外部 API の ID 表現を domain 境界値へ変換する。
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// adapter が外部 API 型へ戻すための ID 表現を借用する。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for BwsProjectId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Bitwarden Secrets Manager secret ID を domain lookup の opaque 値として保持する。
///
/// SDK 型へは依存せず、project 内で一意解決された取得対象を port 境界へ渡す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwsSecretId(String);

impl BwsSecretId {
    /// adapter が外部 API の ID 表現を domain 境界値へ変換する。
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// adapter が外部 API 型へ戻すための ID 表現を借用する。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// BWS SDK から取得した raw secret value の opaque carrier。
/// SDK/backend は bytes を封入するだけで、consumer が wire-format を domain 値へ変換する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwsSecretValue(Vec<u8>);

impl BwsSecretValue {
    pub fn from_bytes(value: impl Into<Vec<u8>>) -> Self {
        Self(value.into())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Bitwarden Secrets Manager の復旧用 project 名。
///
/// project 名の固定値と一意解決規則は、SDK 実装詳細ではなく復旧対象の同一性を表す
/// domain rule として保持する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BwsProjectName;

impl BwsProjectName {
    /// 新規マシン復旧用 BWS project の固定名。
    pub const DOTFILES_SECRET_RECOVERY: Self = Self;

    /// Bitwarden Secrets Manager 上で exact match する固定 project name を返す。
    pub fn as_str(self) -> &'static str {
        "dotfiles-secret-recovery"
    }

    /// BWS project 候補から復旧用 project ID を一意に解決する。
    ///
    /// 0 件と複数件はどちらも domain failure であり、adapter は SDK の候補一覧を渡すだけにする。
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

/// BWS lookup の一意解決に使う候補。
///
/// `id` は adapter が外部 SDK へ戻す opaque 値として扱い、domain は `name` の exact match と
/// 0 件/複数件の失敗条件だけを判定する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwsLookupCandidate<I> {
    pub id: I,
    pub name: String,
}

/// BWS project/secret lookup の exact match を一箇所で失敗条件へ変換する。
///
/// project と secret の両方で「0 件は未登録、複数件は曖昧」という同じ domain rule を共有するため、
/// adapter や application に候補数判定を再実装させない。
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
mod tests {
    use super::{BwsLookupCandidate, BwsProjectName, BwsSecretName};

    fn candidate(id: &'static str, name: &'static str) -> BwsLookupCandidate<&'static str> {
        BwsLookupCandidate {
            id,
            name: name.to_owned(),
        }
    }

    /// BWS project 名の exact match は一意な候補だけを ID として返す。
    #[test]
    fn bws_project_name_resolves_unique_id() {
        let result = BwsProjectName::DOTFILES_SECRET_RECOVERY.resolve_id([
            candidate("other", "other-project"),
            candidate("target", "dotfiles-secret-recovery"),
        ]);

        assert_eq!(result.ok(), Some("target"));
    }

    /// BWS project 名が見つからない場合は domain failure として停止する。
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

    /// BWS project 名が複数候補へ一致する場合は取得対象を曖昧にしない。
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

    /// BWS secret 名の exact match は project 内の一意な候補だけを ID として返す。
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

    /// BWS secret 名が見つからない場合は project ID 付きの domain failure にする。
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

    /// BWS secret 名が複数候補へ一致する場合は secret ID を曖昧にしない。
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
