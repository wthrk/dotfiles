//! Bitwarden 個人 vault の復旧用 secret 同一性を表す domain model。
//!
//! 固定 secret name、一意解決、0 件/複数件の failure 化は SDK 実装詳細ではなく
//! 復旧対象の業務規則であるため、この module に閉じる。

use anyhow::Result;

use crate::support::protection::ProtectedSecret;

/// Bitwarden account API key。
///
/// `client_id` / `client_secret` はどちらも secret material として保持し、argv/stdin/env/log へ出さない。
pub struct BitwardenAccountApiKey {
    client_id: ProtectedSecret,
    client_secret: ProtectedSecret,
}

impl BitwardenAccountApiKey {
    pub fn new(client_id: ProtectedSecret, client_secret: ProtectedSecret) -> Self {
        Self {
            client_id,
            client_secret,
        }
    }

    pub fn client_id(&self) -> &ProtectedSecret {
        &self.client_id
    }

    pub fn client_secret(&self) -> &ProtectedSecret {
        &self.client_secret
    }
}

/// Bitwarden 個人 vault の復号に必要な認証材料。
///
/// YubiKey storage に保存するのは account API key だけで、master password は vault 操作時に
/// CLI/app 側 input port から取得して保持する。
pub struct BitwardenVaultCredentials {
    api_key: BitwardenAccountApiKey,
    master_password: ProtectedSecret,
}

impl BitwardenVaultCredentials {
    pub fn new(api_key: BitwardenAccountApiKey, master_password: ProtectedSecret) -> Self {
        Self {
            api_key,
            master_password,
        }
    }

    pub fn api_key(&self) -> &BitwardenAccountApiKey {
        &self.api_key
    }

    pub fn master_password(&self) -> &ProtectedSecret {
        &self.master_password
    }
}

/// Bitwarden 個人 vault から取得する復旧用 secret 名の閉じた集合。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VaultSecretName {
    GpgSecretKeyBackup,
    PasswordStoreRemote,
}

impl VaultSecretName {
    /// Bitwarden 個人 vault 上で exact match する固定 item 名を返す。
    pub fn key(self) -> &'static str {
        match self {
            Self::GpgSecretKeyBackup => "gpg-secret-key-backup",
            Self::PasswordStoreRemote => "password-store-remote",
        }
    }

    /// 候補を missing / unique / ambiguous の domain 状態へ分類する。
    pub fn resolve_lookup<I: Clone>(
        self,
        candidates: impl IntoIterator<Item = VaultLookupCandidate<I>>,
    ) -> VaultLookupResolution<I> {
        resolve_vault_lookup(candidates, self.key())
    }

    /// 候補から、この復旧対象に一致する ID を一意に解決する。
    pub fn resolve_id<I: Clone>(
        self,
        candidates: impl IntoIterator<Item = VaultLookupCandidate<I>>,
    ) -> Result<I> {
        resolve_single_vault_lookup(
            candidates,
            self.key(),
            || format!("vault secret not found: {}", self.key()),
            || format!("multiple vault secrets matched: {}", self.key()),
        )
    }
}

/// Bitwarden 個人 vault item ID を domain lookup の opaque 値として保持する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultSecretId(String);

impl VaultSecretId {
    /// adapter が外部 API の ID 表現を domain 境界値へ変換する。
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// adapter が外部 API 型へ戻すための ID 表現を借用する。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// vault lookup の一意解決に使う候補。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultLookupCandidate<I> {
    pub id: I,
    pub name: String,
}

/// 固定名 lookup の domain 解決状態。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultLookupResolution<I> {
    Missing,
    Unique(I),
    Ambiguous,
}

fn resolve_single_vault_lookup<I: Clone>(
    candidates: impl IntoIterator<Item = VaultLookupCandidate<I>>,
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

fn resolve_vault_lookup<I: Clone>(
    candidates: impl IntoIterator<Item = VaultLookupCandidate<I>>,
    expected_name: &str,
) -> VaultLookupResolution<I> {
    let mut matches = candidates
        .into_iter()
        .filter(|candidate| candidate.name == expected_name);
    let Some(candidate) = matches.next() else {
        return VaultLookupResolution::Missing;
    };
    if matches.next().is_some() {
        return VaultLookupResolution::Ambiguous;
    }
    VaultLookupResolution::Unique(candidate.id.clone())
}

fn invalid_input(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
/// Bitwarden 個人 vault lookup の domain rule を検証する inline unit test。
mod tests {
    use super::{VaultLookupCandidate, VaultLookupResolution, VaultSecretName};

    fn candidate(id: &'static str, name: &'static str) -> VaultLookupCandidate<&'static str> {
        VaultLookupCandidate {
            id,
            name: name.to_owned(),
        }
    }

    #[test]
    /// 固定 vault secret 名の候補集合を missing / unique / ambiguous に分類する。
    fn vault_secret_name_classifies_lookup_state() {
        assert_eq!(
            VaultSecretName::PasswordStoreRemote.resolve_lookup([candidate("other", "other")]),
            VaultLookupResolution::Missing
        );
        assert_eq!(
            VaultSecretName::PasswordStoreRemote.resolve_lookup([
                candidate("target", "password-store-remote"),
                candidate("other", "other"),
            ]),
            VaultLookupResolution::Unique("target")
        );
        assert_eq!(
            VaultSecretName::PasswordStoreRemote.resolve_lookup([
                candidate("first", "password-store-remote"),
                candidate("second", "password-store-remote"),
            ]),
            VaultLookupResolution::Ambiguous
        );
    }
}
