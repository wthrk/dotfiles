//! `dotfiles secrets` の entrypoint 配線境界。
//!
//! CLI command 定義で parse 済みの入力を application use case へ橋渡しし、
//! composition root が所有する adapter catalog を command ごとの port 引数へ分配する。
//! domain rule と外部 API 翻訳は持たない。

mod dispatch;

use crate::{
    Result,
    features::{
        bws_secrets::ports::public::BwsClientPort,
        cli_interaction::ports::public::{
            BackupUpdateConfirmationPort, BitwardenClientSecretInputPort,
            BootstrapDocumentInputPort, ClockPort, PasswordStoreRemoteInputPort, ReportPort,
            RotationContinuationPort, SecretStorageStatusOutputPort, SshPublicKeyOutputPort,
        },
        gpg_backup_recovery::ports::public::{
            BackupCipherPort, ExportSshPublicKeyCommand, GpgKeyringPort, PrimaryFingerprint,
            SshAgentPort, run_export_ssh_public_key,
        },
        password_store::ports::public::{GitClonePort, PasswordStorePort},
        yubikey_lifecycle::{
            ports::public::{DeviceSerialPort, GpgRecipientPort, SecretStoragePort},
            ports::public::{diagnostics::DiagnosticScopeControl, piv_pin_input::PivPinInputPort},
        },
    },
};

/// composition root が concrete receiver から構築して entrypoint へ渡す trait-reference DTO。
///
/// entrypoint は composition runtime や support/adapter concrete を参照せず、この application-facing
/// contract だけから command ごとの use case を起動する。
pub(crate) struct SecretsInvocation<'a, B>
where
    B: BwsClientPort + ?Sized,
{
    pub(crate) device: &'a mut dyn DeviceSerialPort,
    pub(crate) piv_pin: &'a dyn PivPinInputPort,
    pub(crate) hidden_token_input: &'a dyn BitwardenClientSecretInputPort,
    pub(crate) streamed_token_input: &'a dyn BitwardenClientSecretInputPort,
    pub(crate) hidden_bootstrap_document_input: &'a mut dyn BootstrapDocumentInputPort,
    pub(crate) streamed_bootstrap_document_input: &'a mut dyn BootstrapDocumentInputPort,
    pub(crate) storage: &'a mut dyn SecretStoragePort,
    pub(crate) report: &'a dyn ReportPort,
    pub(crate) bws_client: &'a B,
    pub(crate) gpg_recipient: &'a mut dyn GpgRecipientPort,
    pub(crate) backup_cipher: &'a mut dyn BackupCipherPort,
    pub(crate) gpg_keyring: &'a mut dyn GpgKeyringPort,
    pub(crate) ssh_agent: &'a mut dyn SshAgentPort,
    pub(crate) password_store: &'a mut dyn PasswordStorePort,
    pub(crate) git_clone: &'a mut dyn GitClonePort,
    pub(crate) gpg_agent_socket:
        &'a mut dyn crate::features::gpg_backup_recovery::ports::public::GpgAgentSocketPort,
    pub(crate) status_output: &'a dyn SecretStorageStatusOutputPort,
    pub(crate) rotation_continuation: &'a dyn RotationContinuationPort,
    pub(crate) clock: &'a dyn ClockPort,
    pub(crate) backup_update_confirmation: &'a dyn BackupUpdateConfirmationPort,
    pub(crate) password_store_remote_input: &'a dyn PasswordStoreRemoteInputPort,
    pub(crate) diagnostics: &'a dyn DiagnosticScopeControl,
}

/// parse 済み command を domain command に変換し、composition が生成した port 群で起動する。
///
/// concrete backend の生成・所有は composition に限り、ここは CLI 入力境界と command mapping だけを担う。
pub(crate) async fn start<B>(
    options: crate::SecretsOptions,
    invocation: SecretsInvocation<'_, B>,
) -> Result<()>
where
    B: BwsClientPort + ?Sized,
{
    dispatch::dispatch(options, invocation).await
}

/// `dotfiles gpg` command が借用する application-facing port 群。
pub(crate) struct GpgInvocation<'a> {
    pub(crate) keyring: &'a mut dyn GpgKeyringPort,
    pub(crate) output: &'a dyn SshPublicKeyOutputPort,
}

/// parse 済み `dotfiles gpg` command を domain command へ変換して起動する。
pub(crate) fn start_gpg(options: crate::GpgOptions, invocation: GpgInvocation<'_>) -> Result<()> {
    match options.command {
        crate::GpgCommand::ExportSshPublicKey(options) => {
            let primary_fingerprint = PrimaryFingerprint::parse(&options.primary_fingerprint)?;
            run_export_ssh_public_key(
                ExportSshPublicKeyCommand {
                    primary_fingerprint,
                },
                invocation.keyring,
                invocation.output,
            )
        }
    }
}
