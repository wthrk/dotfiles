//! Bitwarden SDK request / identifier mapping support。

use anyhow::Context;
use uuid::Uuid;

use crate::support::protection::bws;

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
