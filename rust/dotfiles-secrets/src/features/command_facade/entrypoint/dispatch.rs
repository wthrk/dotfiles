//! parse 済み CLI command を domain command に変換し、port contract で application を起動する。
//!
//! concrete backend の生成・所有は composition root に限る。この module はそこで生成済みの runtime
//! から既存 port trait を借用して command 選択と use case 起動だけを行い、support concrete を直接
//! 生成・参照しない。

use crate::{
    Result,
    features::{
        bws_secrets::ports::public::BwsClientPort,
        cli_interaction::ports::public::BootstrapDocumentInputPort,
        gpg_backup_recovery::ports::public::{
            AddGpgBackupSpareCommand, PrimaryFingerprint, RegisterGpgBackupCommand,
            RegisterGpgBackupYubikeyRuntime, RestoreGpgCommand, RestoreGpgIdentityRuntime,
            RestoreGpgYubikeyRuntime, run_add_gpg_backup_spare, run_register_gpg_backup_primary,
            run_restore_gpg, run_validate_gpg_backup_source,
        },
        password_store::ports::public::{
            ProvisionPasswordStoreRemoteCommand, RestorePassCommand, RestorePassRuntime,
            RestorePassYubikeyRuntime, run_provision_password_store_remote,
            run_restore_pass_with_socket,
        },
        provisioning_verification::ports::public::{
            EnrollPrimaryCommand, EnrollSpareCommand, ExternalCheck, ProvisionBwsTokenCommand,
            RotateBwsTokenCommand, VerifyYubikeyCommand, run_enroll_primary, run_enroll_spare,
            run_provision_yubikey_bws_token, run_rotate_bws_token, run_verify_yubikey_with,
        },
        yubikey_lifecycle::{
            ports::public::diagnostics::DiagnosticCommand,
            ports::public::{
                ClearCommand, PutCommand, SetupCommand, StatusCommand, run_clear, run_put,
                run_setup, run_status_with,
            },
        },
    },
};

/// parse 済み command を domain command へ変換し、composition が所有する port を application へ渡す。
pub(super) async fn dispatch<B>(
    options: crate::SecretsOptions,
    invocation: super::SecretsInvocation<'_, B>,
) -> Result<()>
where
    B: BwsClientPort + ?Sized,
{
    match options.command {
        crate::SecretsCommand::Yubikey(options) => match options.command {
            crate::YubikeyCommand::Setup(options) => run_setup(
                SetupCommand {
                    serial: options.serial,
                },
                invocation.device,
                invocation.piv_pin,
                invocation.storage,
            ),
            crate::YubikeyCommand::Clear(options) => run_clear(
                ClearCommand {
                    serial: options.serial,
                    confirmed: options.yes,
                },
                invocation.device,
                invocation.piv_pin,
                invocation.storage,
            ),
            crate::YubikeyCommand::Put(options) => run_put(
                PutCommand::from_cli_name(&options.name, options.serial, options.force)?,
                invocation.device,
                invocation.piv_pin,
                if options.stdin {
                    invocation.streamed_token_input
                } else {
                    invocation.hidden_token_input
                },
                invocation.storage,
            ),
            crate::YubikeyCommand::Status(options) => run_status_with(
                StatusCommand {
                    serial: options.serial,
                },
                invocation.device,
                invocation.storage,
                invocation.status_output,
            ),
            crate::YubikeyCommand::EnrollPrimary(options) => {
                let token = invocation
                    .diagnostics
                    .begin(options.debug, DiagnosticCommand::EnrollPrimary);
                let document_input: &mut dyn BootstrapDocumentInputPort = if options.stdin_json {
                    invocation.streamed_bootstrap_document_input
                } else {
                    invocation.hidden_bootstrap_document_input
                };
                let result = run_enroll_primary(
                    EnrollPrimaryCommand {
                        serial: options.serial,
                    },
                    invocation.device,
                    invocation.piv_pin,
                    document_input,
                    invocation.storage,
                    invocation.report,
                );
                token.finish(&result);
                result
            }
            crate::YubikeyCommand::EnrollSpare(options) => {
                let token = invocation
                    .diagnostics
                    .begin(options.debug, DiagnosticCommand::EnrollSpare);
                let document_input: Option<&mut dyn BootstrapDocumentInputPort> = options
                    .stdin_json
                    .then_some(invocation.streamed_bootstrap_document_input);
                let result = run_enroll_spare(
                    EnrollSpareCommand {
                        primary_serial: options.primary_serial,
                        spare_serial: options.spare_serial,
                    },
                    invocation.device,
                    invocation.piv_pin,
                    document_input,
                    invocation.storage,
                    invocation.report,
                );
                token.finish(&result);
                result
            }
            crate::YubikeyCommand::RotateBwsToken(options) => run_rotate_bws_token(
                RotateBwsTokenCommand {
                    serial: options.serial,
                },
                invocation.device,
                invocation.piv_pin,
                if options.stdin {
                    invocation.streamed_token_input
                } else {
                    invocation.hidden_token_input
                },
                invocation.rotation_continuation,
                invocation.storage,
                invocation.report,
            ),
            crate::YubikeyCommand::ProvisionBwsToken(options) => {
                let token = invocation
                    .diagnostics
                    .begin(options.debug, DiagnosticCommand::ProvisionBwsToken);
                let result = run_provision_yubikey_bws_token(
                    ProvisionBwsTokenCommand {
                        serial: options.serial,
                    },
                    invocation.device,
                    invocation.piv_pin,
                    invocation.hidden_token_input,
                    invocation.storage,
                );
                token.finish(&result);
                result
            }
        },
        crate::SecretsCommand::VerifyYubikey(options) => {
            run_verify_yubikey_with(
                VerifyYubikeyCommand {
                    serial: options.serial,
                    checks: options
                        .check
                        .into_iter()
                        .map(|check| match check {
                            crate::VerifyCheck::Bws => ExternalCheck::Bws,
                        })
                        .collect(),
                    all: options.all,
                },
                invocation.device,
                invocation.storage,
                invocation.report,
                invocation.bws_client,
                invocation.gpg_recipient,
            )
            .await
        }
        crate::SecretsCommand::RestoreGpg(options) => {
            run_restore_gpg(
                RestoreGpgCommand {
                    serial: options.serial,
                },
                RestoreGpgYubikeyRuntime {
                    device: invocation.device,
                    storage: invocation.storage,
                },
                invocation.bws_client,
                invocation.gpg_recipient,
                invocation.backup_cipher,
                RestoreGpgIdentityRuntime {
                    keyring: invocation.gpg_keyring,
                    ssh_agent: invocation.ssh_agent,
                },
                invocation.report,
            )
            .await
        }
        crate::SecretsCommand::RestorePass(options) => {
            run_restore_pass_with_socket(
                RestorePassCommand {
                    serial: options.serial,
                },
                RestorePassRuntime {
                    yubikey: RestorePassYubikeyRuntime {
                        device: invocation.device,
                        storage: invocation.storage,
                    },
                    bws_client: invocation.bws_client,
                    keyring: invocation.gpg_keyring,
                    store: invocation.password_store,
                    git_clone: invocation.git_clone,
                    gpg_agent_socket: invocation.gpg_agent_socket,
                    report: invocation.report,
                },
            )
            .await
        }
        crate::SecretsCommand::GpgBackup(options) => match options.command {
            crate::GpgBackupCommand::Validate(options) => run_validate_gpg_backup_source(
                PrimaryFingerprint::parse(&options.primary_fingerprint)?,
                invocation.gpg_keyring,
            ),
            crate::GpgBackupCommand::Register(options) => {
                run_register_gpg_backup_primary(
                    RegisterGpgBackupCommand {
                        primary_fingerprint: options
                            .primary_fingerprint
                            .as_deref()
                            .map(PrimaryFingerprint::parse)
                            .transpose()?,
                        serial: options.serial,
                    },
                    RegisterGpgBackupYubikeyRuntime {
                        device: invocation.device,
                        storage: invocation.storage,
                    },
                    invocation.gpg_keyring,
                    invocation.backup_cipher,
                    invocation.gpg_recipient,
                    invocation.clock,
                    invocation.bws_client,
                )
                .await
            }
            crate::GpgBackupCommand::AddSpare(options) => {
                run_add_gpg_backup_spare(
                    AddGpgBackupSpareCommand {
                        unwrap_serial: options.unwrap_serial,
                        spare_serial: options.spare_serial,
                        assume_overwrite: options.yes,
                    },
                    invocation.device,
                    invocation.storage,
                    invocation.bws_client,
                    invocation.gpg_recipient,
                    invocation.backup_update_confirmation,
                )
                .await
            }
        },
        crate::SecretsCommand::PassRemote(options) => match options.command {
            crate::PassRemoteCommand::Register(options) => {
                run_provision_password_store_remote(
                    ProvisionPasswordStoreRemoteCommand {
                        assume_overwrite: options.yes,
                        serial: options.serial,
                        url: options.url,
                    },
                    invocation.device,
                    invocation.storage,
                    invocation.bws_client,
                    invocation.password_store_remote_input,
                    invocation.backup_update_confirmation,
                )
                .await
            }
        },
    }
}
