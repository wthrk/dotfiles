//! `dotfiles secrets` の layer-external composition root。
//!
//! production command path が使う concrete backend を生成して所有し、entrypoint へ既存 port trait
//! として貸し出すだけを担う。command の選択、use case の呼出し順序、debug presentation は
//! entrypoint/application に属し、この module はそれらを呼ばない。

use crate::{
    ports::{GpgKeyringPort, SshPublicKeyOutputPort},
    support,
};

/// 一 invocation が借用する production concrete backend 群。
///
/// この型は concrete の生成・所有だけを担う。entrypoint は concrete backend 型を生成せず、ここで
/// 所有される receiver を既存 port contract として application use case へ渡す。
pub(crate) struct SecretsRuntime {
    pub(crate) device: support::yubikey_backend::YubikeyDeviceBackend,
    pub(crate) process_io: support::adapter_backend::ProcessIoBackend,
    pub(crate) hidden_token_input: support::adapter_backend::HiddenTokenInputBackend,
    pub(crate) streamed_token_input: support::adapter_backend::StreamedTokenInputBackend,
    pub(crate) hidden_bootstrap_document_input:
        support::adapter_backend::HiddenBootstrapDocumentInputBackend,
    pub(crate) streamed_bootstrap_document_input:
        support::adapter_backend::StreamedBootstrapDocumentInputBackend,
    pub(crate) storage: support::yubikey_storage::YubikeyStorageBackend,
    pub(crate) report: support::adapter_backend::JsonReportBackend,
    pub(crate) bws_client: support::adapter_backend::BwsClientBackend,
    pub(crate) gpg_recipient: support::yubikey_backend::YubikeyRecipientBackend,
    pub(crate) backup_cipher: support::adapter_backend::BackupCipherBackend,
    pub(crate) gpg_keyring: support::adapter_backend::GpgKeyringBackend,
    pub(crate) ssh_agent: support::adapter_backend::SshAgentBackend,
    pub(crate) password_store: support::adapter_backend::PasswordStoreBackend,
    pub(crate) git_clone: support::adapter_backend::GitCloneBackend,
}

impl SecretsRuntime {
    /// production command path 用の concrete backend を一度だけ生成する。
    pub(crate) fn production() -> Self {
        Self {
            device: support::yubikey_backend::YubikeyDeviceBackend,
            process_io: support::adapter_backend::ProcessIoBackend,
            hidden_token_input: support::adapter_backend::HiddenTokenInputBackend,
            streamed_token_input: support::adapter_backend::StreamedTokenInputBackend,
            hidden_bootstrap_document_input:
                support::adapter_backend::HiddenBootstrapDocumentInputBackend,
            streamed_bootstrap_document_input:
                support::adapter_backend::StreamedBootstrapDocumentInputBackend,
            storage: support::yubikey_storage::YubikeyStorageBackend::default(),
            report: support::adapter_backend::JsonReportBackend,
            bws_client: support::adapter_backend::BwsClientBackend,
            gpg_recipient: support::yubikey_backend::YubikeyRecipientBackend,
            backup_cipher: support::adapter_backend::BackupCipherBackend,
            gpg_keyring: support::adapter_backend::GpgKeyringBackend,
            ssh_agent: support::adapter_backend::SshAgentBackend,
            password_store: support::adapter_backend::PasswordStoreBackend,
            git_clone: support::adapter_backend::GitCloneBackend,
        }
    }
}

/// `dotfiles gpg` が借用する concrete backend 群。
///
/// public-key export の command mapping は entrypoint に残し、この型は keyring と output port の
/// concrete 生成・所有だけを担当する。
pub(crate) struct GpgRuntime {
    keyring: support::adapter_backend::GpgKeyringBackend,
    output: support::adapter_backend::ProcessIoBackend,
}

impl GpgRuntime {
    pub(crate) fn production() -> Self {
        Self {
            keyring: support::adapter_backend::GpgKeyringBackend,
            output: support::adapter_backend::ProcessIoBackend,
        }
    }

    pub(crate) fn ports(&mut self) -> (&mut dyn GpgKeyringPort, &dyn SshPublicKeyOutputPort) {
        (&mut self.keyring, &self.output)
    }
}
