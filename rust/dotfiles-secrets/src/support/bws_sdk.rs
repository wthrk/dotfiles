//! Bitwarden SDK request / identifier mapping support。

use anyhow::Context;
use bitwarden::secrets_manager::secrets::SecretCreateRequest;
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

pub(crate) fn secret_create_request(
    organization_id: Uuid,
    project_id: Uuid,
    key: &str,
    value: String,
) -> SecretCreateRequest {
    SecretCreateRequest {
        organization_id,
        key: key.to_owned(),
        value,
        note: String::new(),
        project_ids: Some(vec![project_id]),
    }
}
