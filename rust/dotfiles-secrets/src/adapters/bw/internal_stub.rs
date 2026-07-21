//! `secrets-internal-test-stub` feature の BWS port translation。
//!
//! fixture datastore、spec JSON と observation serialization は `support::internal_stub_bws` が
//! 所有する。この adapter は primitive state operation を BWS port の domain contract に翻訳するだけである。

use crate::{
    domain::{
        bws::{BwsLookupCandidate, BwsProjectId, BwsSecretId, BwsSecretName},
        gpg_backup::{BackupUpdateGuard, GpgBackupEnvelope},
        pass_restore::PasswordStoreRemote,
    },
    ports::bw::BwsClientPort,
    support::{adapter_backend::BwsClientBackend, internal_stub_bws, protection::ProtectedSecret},
};

impl BwsClientPort for BwsClientBackend {
    async fn list_bws_projects(
        &self,
        access_token: &ProtectedSecret,
    ) -> crate::Result<Vec<BwsLookupCandidate<BwsProjectId>>> {
        Ok(
            internal_stub_bws::list_projects(&access_token.to_test_bytes())?
                .into_iter()
                .map(|(id, name)| BwsLookupCandidate {
                    id: BwsProjectId::new(id),
                    name,
                })
                .collect(),
        )
    }

    async fn list_bws_secrets(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
    ) -> crate::Result<Vec<BwsLookupCandidate<BwsSecretId>>> {
        Ok(internal_stub_bws::list_project_secrets(
            &access_token.to_test_bytes(),
            project_id.as_str(),
        )?
        .into_iter()
        .map(|(id, name)| BwsLookupCandidate {
            id: BwsSecretId::new(id),
            name,
        })
        .collect())
    }

    async fn fetch_gpg_backup_envelope(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<(GpgBackupEnvelope, BackupUpdateGuard)> {
        let value =
            internal_stub_bws::read_secret(&access_token.to_test_bytes(), secret_id.as_str())?;
        Ok((
            GpgBackupEnvelope::from_json(value.as_bytes())?,
            BackupUpdateGuard::from_value_bytes(value.as_bytes()),
        ))
    }

    async fn fetch_password_store_remote(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<PasswordStoreRemote> {
        PasswordStoreRemote::parse(&internal_stub_bws::read_secret(
            &access_token.to_test_bytes(),
            secret_id.as_str(),
        )?)
    }

    async fn create_gpg_backup_envelope(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
        envelope: &GpgBackupEnvelope,
    ) -> crate::Result<BwsSecretId> {
        let value = String::from_utf8(envelope.to_json()?)
            .map_err(|_| anyhow::anyhow!("gpg backup envelope is not valid UTF-8"))?;
        Ok(BwsSecretId::new(internal_stub_bws::create_secret(
            &access_token.to_test_bytes(),
            project_id.as_str(),
            BwsSecretName::GpgSecretKeyBackup.key(),
            value,
        )?))
    }

    async fn update_gpg_backup_envelope_if_unchanged(
        &self,
        access_token: &ProtectedSecret,
        _project_id: &BwsProjectId,
        secret_id: &BwsSecretId,
        envelope: &GpgBackupEnvelope,
        expected_guard: &BackupUpdateGuard,
    ) -> crate::Result<()> {
        let token = access_token.to_test_bytes();
        let current = internal_stub_bws::read_secret(&token, secret_id.as_str())?;
        expected_guard.ensure_matches(&BackupUpdateGuard::from_value_bytes(current.as_bytes()))?;
        let value = String::from_utf8(envelope.to_json()?)
            .map_err(|_| anyhow::anyhow!("gpg backup envelope is not valid UTF-8"))?;
        internal_stub_bws::replace_secret(&token, secret_id.as_str(), value)?;
        Ok(())
    }

    async fn create_password_store_remote(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
        remote: &PasswordStoreRemote,
    ) -> crate::Result<BwsSecretId> {
        Ok(BwsSecretId::new(internal_stub_bws::create_secret(
            &access_token.to_test_bytes(),
            project_id.as_str(),
            BwsSecretName::PasswordStoreRemote.key(),
            remote.as_str().to_owned(),
        )?))
    }

    async fn fetch_password_store_remote_guard(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<BackupUpdateGuard> {
        let value =
            internal_stub_bws::read_secret(&access_token.to_test_bytes(), secret_id.as_str())?;
        Ok(BackupUpdateGuard::from_value_bytes(value.as_bytes()))
    }

    async fn update_password_store_remote_if_unchanged(
        &self,
        access_token: &ProtectedSecret,
        _project_id: &BwsProjectId,
        secret_id: &BwsSecretId,
        remote: &PasswordStoreRemote,
        expected_guard: &BackupUpdateGuard,
    ) -> crate::Result<()> {
        let token = access_token.to_test_bytes();
        let current = internal_stub_bws::read_secret(&token, secret_id.as_str())?;
        expected_guard.ensure_matches(&BackupUpdateGuard::from_value_bytes(current.as_bytes()))?;
        internal_stub_bws::replace_secret(&token, secret_id.as_str(), remote.as_str().to_owned())?;
        Ok(())
    }
}
