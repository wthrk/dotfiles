//! parse 済み CLI command を application use case へ橋渡しする dispatch 境界。
//!
//! command ごとの application 呼び出し順序だけを担い、adapter 生成と support session は
//! composition / runtime 境界へ分離する。外部 API 翻訳は adapter 側へ閉じる。

use crate::{
    Result, application,
    domain::{
        commands::{
            AddGpgBackupSpareCommand, BwLoginCommand, ClearCommand, EnrollPrimaryCommand,
            EnrollSpareCommand, ProvisionPasswordStoreRemoteCommand, PutCommand,
            RegisterGpgBackupCommand, RestoreGpgCommand, RestorePassCommand, RotateBwsTokenCommand,
            SetupCommand, StatusCommand, VerifyYubikeyCommand,
        },
        gpg_backup::PrimaryFingerprint,
        verification::ExternalCheck,
    },
};

/// parse 済み command を use case に橋渡しする。
pub(super) async fn dispatch(
    options: super::super::SecretsOptions,
    ports: &mut super::super::RuntimePorts,
) -> Result<()> {
    match options.command {
        super::super::SecretsCommand::Yubikey(options) => match options.command {
            super::super::YubikeyCommand::Setup(options) => {
                application::run_setup_with::run_setup_with(
                    SetupCommand {
                        serial: options.serial,
                    },
                    &mut ports.device,
                    &mut ports.storage,
                )
            }
            super::super::YubikeyCommand::Clear(options) => application::run_clear_with::run_clear_with(
                ClearCommand {
                    serial: options.serial,
                    confirmed: options.yes,
                },
                &mut ports.device,
                &mut ports.storage,
            ),
            super::super::YubikeyCommand::Put(options) => {
                let command = PutCommand {
                    name: options.name,
                    serial: options.serial,
                    force: options.force,
                };
                if options.stdin {
                    application::run_put_with_stdin::run_put_with_stdin(
                        command,
                        &mut ports.device,
                        &ports.process_io,
                        &mut ports.storage,
                    )
                } else {
                    application::run_put_with_prompt::run_put_with_prompt(
                        command,
                        &mut ports.device,
                        &ports.process_io,
                        &mut ports.storage,
                    )
                }
            }
            super::super::YubikeyCommand::Status(options) => application::run_status_with::run_status_with(
                StatusCommand {
                    serial: options.serial,
                },
                &mut ports.device,
                &mut ports.storage,
                &ports.process_io,
            ),
            super::super::YubikeyCommand::EnrollPrimary(options) => {
                let command = EnrollPrimaryCommand {
                    serial: options.serial,
                };
                if options.stdin_json {
                    application::run_enroll_primary_with_stdin_json::run_enroll_primary_with_stdin_json(
                        command,
                        &mut ports.device,
                        &ports.process_io,
                        &mut ports.storage,
                        &ports.report,
                    )
                } else {
                    application::run_enroll_primary_with_prompt::run_enroll_primary_with_prompt(
                        command,
                        &mut ports.device,
                        &ports.process_io,
                        &mut ports.storage,
                        &ports.report,
                    )
                }
            }
            super::super::YubikeyCommand::EnrollSpare(options) => {
                let command = EnrollSpareCommand {
                    primary_serial: options.primary_serial,
                    spare_serial: options.spare_serial,
                };
                if options.stdin_json {
                    application::run_enroll_spare_with_stdin_json::run_enroll_spare_with_stdin_json(
                        command,
                        &mut ports.device,
                        &ports.process_io,
                        &mut ports.storage,
                        &ports.report,
                    )
                } else {
                    application::run_enroll_spare_with_prompt::run_enroll_spare_with_prompt(
                        command,
                        &mut ports.device,
                        &mut ports.storage,
                        &ports.report,
                    )
                }
            }
            super::super::YubikeyCommand::RotateBwsToken(options) => {
                let command = RotateBwsTokenCommand {
                    serial: options.serial,
                };
                if options.stdin {
                    application::run_rotate_bws_token_with_stdin::run_rotate_bws_token_with_stdin(
                        command,
                        &mut ports.device,
                        &ports.process_io,
                        &mut ports.storage,
                        &ports.report,
                    )
                } else {
                    application::run_rotate_bws_token_with_prompt::run_rotate_bws_token_with_prompt(
                        command,
                        application::run_rotate_bws_token_with_prompt::RotateBwsTokenPromptRuntime {
                            device: &mut ports.device,
                            secret_input: &ports.process_io,
                            continuation: &ports.process_io,
                            storage: &mut ports.storage,
                            report: &ports.report,
                        },
                    )
                }
            }
        },
        super::super::SecretsCommand::VerifyYubikey(options) => {
            application::run_verify_yubikey_with::run_verify_yubikey_with(
                VerifyYubikeyCommand {
                    serial: options.serial,
                    checks: options
                        .check
                        .into_iter()
                        .map(|check| match check {
                            super::super::VerifyCheck::Bws => ExternalCheck::Bws,
                            super::super::VerifyCheck::BwLogin => ExternalCheck::BwLogin,
                        })
                        .collect(),
                    all: options.all,
                    email_override: options.email,
                },
                application::run_verify_yubikey_with::VerifyYubikeyRuntime {
                    device: &mut ports.device,
                    storage: &mut ports.storage,
                    report: &ports.report,
                    bws_client: &ports.bws_client,
                    gpg_recipient: &mut ports.gpg_recipient,
                    otp_input: &ports.process_io,
                    bw_login: &ports.bw_login,
                },
            )
            .await
        }
        super::super::SecretsCommand::BwLogin(options) => {
            application::run_bw_login::run_bw_login(
                BwLoginCommand {
                    serial: options.serial,
                    email_override: options.email,
                },
                application::run_bw_login::BwLoginRuntime {
                    device: &mut ports.device,
                    storage: &mut ports.storage,
                    otp_input: &ports.process_io,
                    bw_login: &ports.bw_login,
                    report: &ports.report,
                },
            )
            .await
        }
        super::super::SecretsCommand::RestoreGpg(options) => {
            application::run_restore_gpg::run_restore_gpg(
                RestoreGpgCommand {
                    serial: options.serial,
                },
                application::run_restore_gpg::RestoreGpgRuntime {
                    device: &mut ports.device,
                    storage: &mut ports.storage,
                    bws_client: &ports.bws_client,
                    recipient: &mut ports.gpg_recipient,
                    cipher: &mut ports.backup_cipher,
                    keyring: &mut ports.gpg_keyring,
                    ssh_agent: &mut ports.ssh_agent,
                    report: &ports.report,
                },
            )
            .await
        }
        super::super::SecretsCommand::RestorePass(options) => {
            application::run_restore_pass::run_restore_pass(
                RestorePassCommand {
                    serial: options.serial,
                },
                application::run_restore_pass::RestorePassRuntime {
                    device: &mut ports.device,
                    storage: &mut ports.storage,
                    bws_client: &ports.bws_client,
                    keyring: &mut ports.gpg_keyring,
                    store: &mut ports.password_store,
                    git_clone: &mut ports.git_clone,
                    report: &ports.report,
                },
            )
            .await
        }
        super::super::SecretsCommand::GpgBackup(options) => match options.command {
            super::super::GpgBackupCommand::Register(options) => {
                let primary_fingerprint = options.primary_fingerprint
                    .as_deref()
                    .map(PrimaryFingerprint::parse)
                    .transpose()?;
                application::run_register_gpg_backup_primary::run_register_gpg_backup_primary(
                    RegisterGpgBackupCommand {
                        primary_fingerprint,
                        serial: options.serial,
                    },
                    application::run_register_gpg_backup_primary::RegisterGpgBackupPrimaryRuntime {
                        device: &mut ports.device,
                        storage: &mut ports.storage,
                        keyring: &mut ports.gpg_keyring,
                        cipher: &mut ports.backup_cipher,
                        recipient: &mut ports.gpg_recipient,
                        clock: &ports.process_io,
                        bws_client: &ports.bws_client,
                    },
                )
                .await
            }
            super::super::GpgBackupCommand::AddSpare(options) => {
                application::run_add_gpg_backup_spare::run_add_gpg_backup_spare(
                    AddGpgBackupSpareCommand {
                        unwrap_serial: options.unwrap_serial,
                        spare_serial: options.spare_serial,
                        assume_overwrite: options.yes,
                    },
                    application::run_add_gpg_backup_spare::AddGpgBackupSpareRuntime {
                        device: &mut ports.device,
                        storage: &mut ports.storage,
                        bws_client: &ports.bws_client,
                        recipient: &mut ports.gpg_recipient,
                        confirmation: &ports.process_io,
                    },
                )
                .await
            }
        },
        super::super::SecretsCommand::PassRemote(options) => match options.command {
            super::super::PassRemoteCommand::Register(options) => {
                application::run_provision_password_store_remote::run_provision_password_store_remote(
                    ProvisionPasswordStoreRemoteCommand {
                        assume_overwrite: options.yes,
                        serial: options.serial,
                        url: options.url,
                    },
                    &mut ports.device,
                    &mut ports.storage,
                    &ports.bws_client,
                    &ports.process_io,
                    &ports.process_io,
                )
                .await
            }
        },
    }
}
