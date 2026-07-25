//! Bitwarden SDK request / identifier mapping support。
//!
//! ## 出典と適用判断
//!
//! repository 正本は [`secret-recovery-spec.md` の「Bitwarden Secrets Manager」](../../../docs/secret-recovery/secret-recovery-spec.md#bitwarden-secrets-manager)
//! と [`bitwarden-personal-vault-design.md`](../../../docs/secret-recovery/bitwarden-personal-vault-design.md)
//! である。BWS token は YubiKey storage からのみ得て、project / secret の業務上の一意解決は
//! application/domain が行う。この module は SDK organization ID と repository の UUID boundary を
//! 接続するだけである。
//!
//! vendor の access-token と machine-account の project 権限は
//! [Bitwarden Secrets Manager SDK](https://bitwarden.com/help/secrets-manager-sdk/) と
//! [Machine Accounts](https://bitwarden.com/help/machine-accounts/) を直接確認する。固定 SDK source は
//! `bitwarden-sm` 3.0.0
//! [`SecretsManagerClient::get_access_token_organization`](https://docs.rs/crate/bitwarden-sm/3.0.0/source/src/client.rs)
//! であり、戻り値は `Option<OrganizationId>` である。
//!
//! `Some` だけを BWS scope ID として渡し、`None` は scope が得られない failure として停止する。
//! default organization、空 UUID、別 project の探索で補完しない。`parse_uuid` の failure は SDK error
//! ではなく repository boundary の malformed-ID failure であり、同様に補完せず伝播する。

use anyhow::Context;
use uuid::Uuid;

use super::protection_bws as bws;

pub(crate) fn access_token_scope_id(session: &bws::BwsClientSession) -> crate::Result<Uuid> {
    session
        .client()
        .get_access_token_organization()
        .map(Into::into)
        .ok_or_else(|| anyhow::anyhow!("bitwarden access token does not expose a BWS SDK scope id"))
}

pub(crate) fn parse_uuid(value: &str, label: &str) -> crate::Result<Uuid> {
    value
        .parse()
        .with_context(|| format!("{label} is not a valid UUID"))
}
