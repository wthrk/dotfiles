//! BWS port の forwarding-only adapter。

#[cfg(not(feature = "secrets-internal-test-stub"))]
use crate::{
    domain::{
        bws::{BwsLookupCandidate, BwsProjectId, BwsSecretId},
        gpg_backup::{BackupUpdateGuard, GpgBackupEnvelope},
        pass_restore::PasswordStoreRemote,
    },
    ports::bw::BwsClientPort,
    support::{adapter_backend::BwsClientBackend, bws_backend, protection::ProtectedSecret},
};

#[cfg(not(feature = "secrets-internal-test-stub"))]
impl BwsClientPort for BwsClientBackend {
    async fn list_bws_projects(
        &self,
        access_token: &ProtectedSecret,
    ) -> crate::Result<Vec<BwsLookupCandidate<BwsProjectId>>> {
        bws_backend::list_bws_projects(access_token).await
    }
    async fn list_bws_secrets(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
    ) -> crate::Result<Vec<BwsLookupCandidate<BwsSecretId>>> {
        bws_backend::list_bws_secrets(access_token, project_id).await
    }
    async fn fetch_gpg_backup_envelope(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<(GpgBackupEnvelope, BackupUpdateGuard)> {
        bws_backend::fetch_gpg_backup_envelope(access_token, secret_id).await
    }
    async fn fetch_password_store_remote(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<PasswordStoreRemote> {
        bws_backend::fetch_password_store_remote(access_token, secret_id).await
    }
    async fn create_gpg_backup_envelope(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
        secret_key: &str,
        envelope: &GpgBackupEnvelope,
    ) -> crate::Result<BwsSecretId> {
        bws_backend::create_gpg_backup_envelope(access_token, project_id, secret_key, envelope)
            .await
    }
    async fn update_gpg_backup_envelope_if_unchanged(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
        secret_id: &BwsSecretId,
        secret_key: &str,
        envelope: &GpgBackupEnvelope,
        expected_guard: &BackupUpdateGuard,
    ) -> crate::Result<()> {
        bws_backend::update_gpg_backup_envelope_if_unchanged(
            access_token,
            project_id,
            secret_id,
            secret_key,
            envelope,
            expected_guard,
        )
        .await
    }
    async fn create_password_store_remote(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
        secret_key: &str,
        remote: &PasswordStoreRemote,
    ) -> crate::Result<BwsSecretId> {
        bws_backend::create_password_store_remote(access_token, project_id, secret_key, remote)
            .await
    }
    async fn fetch_password_store_remote_guard(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<BackupUpdateGuard> {
        bws_backend::fetch_password_store_remote_guard(access_token, secret_id).await
    }
    async fn update_password_store_remote_if_unchanged(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
        secret_id: &BwsSecretId,
        secret_key: &str,
        remote: &PasswordStoreRemote,
        expected_guard: &BackupUpdateGuard,
    ) -> crate::Result<()> {
        bws_backend::update_password_store_remote_if_unchanged(
            access_token,
            project_id,
            secret_id,
            secret_key,
            remote,
            expected_guard,
        )
        .await
    }
}
