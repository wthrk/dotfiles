//! BWS SDK が要求する所有 plaintext buffer と secret 返却値を protection 境界内で扱う操作。
#![cfg_attr(feature = "secrets-internal-test-stub", allow(dead_code))]

#[cfg(not(feature = "secrets-internal-test-stub"))]
use bitwarden::auth::AccessToken;
use bitwarden::{
    Client, auth::login::AccessTokenLoginRequest, secrets_manager::secrets::SecretGetRequest,
};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use super::SecretSession;
use crate::{Result, secrets::support::protection::ProtectedSecret};

const PROVISIONING_TOKEN_NOTE_PREFIX: &str = "dotfiles-provisioning-bws-access-token-id=";

/// SDK login request の repository 所有 access token buffer を Drop で zeroize する guard。
///
/// caller はこの guard から借用した request を SDK 呼び出し境界内だけで使う。future が
/// panic/unwind した場合でも guard の Drop が走る限り `access_token` buffer を zeroize する。
struct ZeroizingAccessTokenLoginRequest {
    inner: AccessTokenLoginRequest,
}

/// core dump 抑止を確立した BWS SDK client session。
///
/// BWS SDK の login/list/get 境界へ入る前に `SecretSession` を開始し、access token と返却 secret
/// value を扱う SDK 呼び出し期間中は process-wide core dump 抑止済みであることを型で保持する。
pub(crate) struct BwsClientSession {
    client: Client,
    _session: SecretSession,
}

impl ZeroizingAccessTokenLoginRequest {
    fn new(access_token: String) -> Self {
        Self {
            inner: AccessTokenLoginRequest {
                access_token,
                state_file: None,
            },
        }
    }

    fn as_request(&self) -> &AccessTokenLoginRequest {
        &self.inner
    }
}

impl Drop for ZeroizingAccessTokenLoginRequest {
    fn drop(&mut self) {
        self.inner.access_token.zeroize();
    }
}

impl BwsClientSession {
    /// SDK client を借用し、BWS list/get 呼び出しを session lifetime 内で実行させる。
    pub(crate) fn client(&self) -> &Client {
        &self.client
    }

    /// SDK get で取得した secret value を protected buffer へ移し、revision と同じ borrow 境界で処理する。
    ///
    /// 返却 secret の plaintext はこの protection backend 操作内に閉じ、caller へ `ProtectedSecret` の
    /// 汎用 borrow API を渡さない。caller は closure で検証済み value または error だけを返す。
    pub(crate) async fn parse_secret_value_with_revision<R>(
        &self,
        id: Uuid,
        parse: impl FnOnce(&str, &str) -> Result<R>,
    ) -> Result<R> {
        let secret = self
            .client
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .map_err(|_| anyhow::anyhow!("bitwarden secret get failed"))?;
        let revision = secret.revision_date.to_rfc3339();
        let value = Zeroizing::new(secret.value);
        let mut protected = ProtectedSecret::new(value.len())?;
        protected.with_secret_mut(|out| out.copy_from_slice(value.as_bytes()));
        protected.with_secret_utf8_protection(|json| parse(json, revision.as_str()))
    }

    /// SDK get で取得した secret value を protected buffer 外へ出せる `Zeroizing` 管理の文字列として返す。
    ///
    /// caller は対象値が protected borrow 境界を必要としないことを protection 外で確定してから呼び出す。
    /// この support 境界は BWS SDK の secret value を repository 所有文字列へ移し、Drop 時の zeroize を
    /// 保証するだけで、保存モデル上の secret 名・用途判断は持たない。
    pub(crate) async fn get_non_secret_value(&self, id: Uuid) -> Result<Zeroizing<String>> {
        let secret = self
            .client
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .map_err(|_| anyhow::anyhow!("bitwarden secret get failed"))?;
        Ok(Zeroizing::new(secret.value))
    }

    /// SDK get で取得した secret note を zeroize 管理の文字列として返す。
    pub(crate) async fn get_non_secret_note(&self, id: Uuid) -> Result<Zeroizing<String>> {
        let secret = self
            .client
            .secrets()
            .get(&SecretGetRequest { id })
            .await
            .map_err(|_| anyhow::anyhow!("bitwarden secret get failed"))?;
        Ok(Zeroizing::new(secret.note))
    }
}

/// BWS access token で SDK 認証済み client を作成する。
///
/// ここでは SDK が要求する所有 token buffer の作成、login 呼び出し、repository 所有 buffer の
/// zeroize だけを完了する。project/secret lookup rule や外部確認 plan は扱わない。
pub(crate) async fn login_client_with_access_token(
    access_token: &ProtectedSecret,
) -> Result<BwsClientSession> {
    let session = SecretSession::start()?;
    let client = access_token
        .with_secret_utf8_async(|token| {
            Box::pin(async {
                let client = Client::new(None);
                let request = ZeroizingAccessTokenLoginRequest::new(token.trim().to_owned());
                client
                    .auth()
                    .login_access_token(request.as_request())
                    .await
                    .map_err(|_| anyhow::anyhow!("bitwarden login failed"))?;
                drop(request);
                Ok(client)
            })
        })
        .await?;
    Ok(BwsClientSession {
        client,
        _session: session,
    })
}

/// BWS access token の plaintext から opaque provisioning token id だけを抽出する。
///
/// token 値全体は protection 境界から出さず、YubiKey 保存前の provenance 比較に必要な非機密 id だけを返す。
pub(crate) fn provisioning_token_id(access_token: &ProtectedSecret) -> Result<String> {
    #[cfg(feature = "secrets-internal-test-stub")]
    {
        let raw = access_token.to_test_bytes();
        let mut bytes = [0_u8; 16];
        if raw.is_empty() {
            bytes[15] = 1;
        } else {
            for (index, slot) in bytes.iter_mut().enumerate() {
                let source = raw[index % raw.len()];
                *slot = source ^ (index as u8).wrapping_mul(17);
            }
        }
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        return Ok(Uuid::from_bytes(bytes).to_string());
    }

    #[cfg(not(feature = "secrets-internal-test-stub"))]
    access_token.with_secret_utf8(|token| {
        let parsed: AccessToken = token.trim().parse().map_err(|_| {
            anyhow::anyhow!("bitwarden access token is not a supported BWS access token")
        })?;
        Ok(parsed.access_token_id.to_string())
    })
}

/// provisioning command が `password-store-remote` note に残す非機密 provenance marker を組み立てる。
pub(crate) fn provisioning_token_note(access_token: &ProtectedSecret) -> Result<String> {
    Ok(format!(
        "{PROVISIONING_TOKEN_NOTE_PREFIX}{}",
        provisioning_token_id(access_token)?
    ))
}

/// `password-store-remote` note から provenance marker を取り出し、欠落や改ざんを fail-closed で弾く。
pub(crate) fn parse_provisioning_token_note(note: &str) -> Option<&str> {
    let token_id = note.strip_prefix(PROVISIONING_TOKEN_NOTE_PREFIX)?;
    if token_id.is_empty() {
        return None;
    }
    let parsed: Uuid = token_id.parse().ok()?;
    if parsed.to_string() != token_id {
        return None;
    }
    Some(token_id)
}

/// 候補 recovery token が provisioning token と同一でないことを protection 境界内で確認する。
pub(crate) fn ensure_recovery_token_allowed(
    access_token: &ProtectedSecret,
    note_marker_id: Option<&str>,
) -> Result<()> {
    let candidate_token_id = provisioning_token_id(access_token)?;
    match note_marker_id {
        Some(existing) if existing == candidate_token_id => {
            anyhow::bail!(
                "refusing to store bws-access-token: recovery token must differ from the provisioning token"
            );
        }
        Some(_) => Ok(()),
        None => anyhow::bail!(
            "refusing to store bws-access-token: password-store-remote is missing provisioning token provenance"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ensure_recovery_token_allowed, parse_provisioning_token_note, provisioning_token_note,
    };
    use crate::secrets::support::protection::ProtectedSecret;

    fn test_secret() -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(
            b"0.ec2c1d46-6a4b-4751-a310-af9601317f2d.C2IgxjjLF7qSshsbwe8JGcbM075YXw:X8vbvA0bduihIDe/qrzIQQ==",
        )
        .expect("test secret")
    }

    #[test]
    fn provisioning_note_round_trips() {
        let secret = test_secret();
        let note = provisioning_token_note(&secret).expect("note");

        let marker = parse_provisioning_token_note(&note).expect("marker");
        assert_eq!(marker, "ec2c1d46-6a4b-4751-a310-af9601317f2d");
    }

    #[test]
    fn recovery_token_gate_rejects_same_marker() {
        let secret = test_secret();
        let note = provisioning_token_note(&secret).expect("note");

        let error = ensure_recovery_token_allowed(&secret, parse_provisioning_token_note(&note))
            .expect_err("same token id must be rejected");

        assert_eq!(
            error.to_string(),
            "refusing to store bws-access-token: recovery token must differ from the provisioning token"
        );
    }

    #[test]
    fn recovery_token_gate_rejects_missing_marker() {
        let secret = test_secret();

        let error = ensure_recovery_token_allowed(&secret, None)
            .expect_err("missing marker must fail closed");

        assert_eq!(
            error.to_string(),
            "refusing to store bws-access-token: password-store-remote is missing provisioning token provenance"
        );
    }
}
