//! `secrets-internal-test-stub` BWS port の forwarding-only adapter。

use crate::{
    features::bws_secrets::ports::bw::BwsClientPort,
    features::bws_secrets::support::internal_stub_bws,
    features::{
        bws_secrets::domain::bws::{BwsLookupCandidate, BwsProjectId, BwsSecretId},
        gpg_backup_recovery::ports::public::{BackupUpdateGuard, GpgBackupEnvelope},
        password_store::ports::public::PasswordStoreRemote,
    },
    foundation::protection::ProtectedSecret,
    shared::contracts::adapter_backend::BwsClientBackend,
};

impl BwsClientPort for BwsClientBackend {
    async fn list_bws_projects(
        &self,
        access_token: &ProtectedSecret,
    ) -> crate::Result<Vec<BwsLookupCandidate<BwsProjectId>>> {
        internal_stub_bws::list_bws_projects(access_token).await
    }
    async fn list_bws_secrets(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
    ) -> crate::Result<Vec<BwsLookupCandidate<BwsSecretId>>> {
        internal_stub_bws::list_bws_secrets(access_token, project_id).await
    }
    async fn fetch_gpg_backup_envelope(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<(GpgBackupEnvelope, BackupUpdateGuard)> {
        internal_stub_bws::fetch_gpg_backup_envelope(access_token, secret_id).await
    }
    async fn fetch_password_store_remote(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<PasswordStoreRemote> {
        internal_stub_bws::fetch_password_store_remote(access_token, secret_id).await
    }
    async fn create_gpg_backup_envelope(
        &self,
        access_token: &ProtectedSecret,
        project_id: &BwsProjectId,
        secret_key: &str,
        envelope: &GpgBackupEnvelope,
    ) -> crate::Result<BwsSecretId> {
        internal_stub_bws::create_gpg_backup_envelope(
            access_token,
            project_id,
            secret_key,
            envelope,
        )
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
        internal_stub_bws::update_gpg_backup_envelope_if_unchanged(
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
        internal_stub_bws::create_password_store_remote(
            access_token,
            project_id,
            secret_key,
            remote,
        )
        .await
    }
    async fn fetch_password_store_remote_guard(
        &self,
        access_token: &ProtectedSecret,
        secret_id: &BwsSecretId,
    ) -> crate::Result<BackupUpdateGuard> {
        internal_stub_bws::fetch_password_store_remote_guard(access_token, secret_id).await
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
        internal_stub_bws::update_password_store_remote_if_unchanged(
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
