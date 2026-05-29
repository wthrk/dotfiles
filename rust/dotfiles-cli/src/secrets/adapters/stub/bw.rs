//! `secrets-internal-test-stub` feature 専用の file-backed BWS backend。
//! production build には含めず、state file を通じて integration test から観測する。

use crate::secrets::{
    domain::values::{BwsLookupCandidate, BwsProjectId, BwsSecretId},
    support::protection::ProtectedSecret,
};

use super::state::{StubState, with_state};

pub(super) fn list_bws_projects(
    access_token: &ProtectedSecret,
) -> crate::Result<Vec<BwsLookupCandidate<BwsProjectId>>> {
    with_state(|state| {
        ensure_access_token_matches_state(access_token, state)?;
        let mut out = Vec::with_capacity(state.bws_projects.len());
        for (project_id, project_name) in &state.bws_projects {
            out.push(BwsLookupCandidate {
                id: BwsProjectId::new(project_id.clone()),
                name: project_name.clone(),
            });
        }
        Ok(out)
    })
}

pub(super) fn list_bws_secrets(
    access_token: &ProtectedSecret,
    project_id: &BwsProjectId,
) -> crate::Result<Vec<BwsLookupCandidate<BwsSecretId>>> {
    with_state(|state| {
        ensure_access_token_matches_state(access_token, state)?;
        let candidates = state
            .bws_project_secrets
            .get(project_id.as_str())
            .ok_or_else(|| anyhow::anyhow!("bitwarden project not found"))?;
        let mut out = Vec::with_capacity(candidates.len());
        for (secret_id, secret_name) in candidates {
            out.push(BwsLookupCandidate {
                id: BwsSecretId::new(secret_id.clone()),
                name: secret_name.clone(),
            });
        }
        Ok(out)
    })
}

pub(super) fn fetch_bws_secret_by_id(
    access_token: &ProtectedSecret,
    secret_id: &BwsSecretId,
) -> crate::Result<ProtectedSecret> {
    with_state(|state| {
        ensure_access_token_matches_state(access_token, state)?;
        let bytes = state
            .bws_secret_values
            .get(secret_id.as_str())
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("bitwarden secret get failed"))?;
        state.bws_fetch_events.push(format!(
            "DOTFILES_TEST_BWS_FETCH id={} bytes={}",
            secret_id.as_str(),
            bytes.len()
        ));
        let session = crate::secrets::support::protection::SecretSession::start()?;
        let buffer =
            crate::secrets::support::protection::buffer::ProtectedInputBuffer::read_line_from(
                std::io::Cursor::new(bytes),
                16 * 1024,
                &session,
            )?;
        buffer
            .into_protected_secret_line(&session, 16 * 1024, "internal stub secret is too large")
            .map_err(Into::into)
    })
}

fn ensure_access_token_matches_state(
    access_token: &ProtectedSecret,
    state: &StubState,
) -> crate::Result<()> {
    let configured = state
        .bws_secret_values
        .get("bws-secret-id-access-token")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("bws access token stub secret is not configured"))?;
    if access_token.to_test_bytes() == configured {
        Ok(())
    } else {
        anyhow::bail!("bitwarden login failed")
    }
}
