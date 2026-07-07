//! parse 済み CLI command を application use case へ橋渡しする dispatch 境界。
//!
//! command ごとの application 呼び出し順序だけを担い、adapter 生成と support session は
//! composition / runtime 境界へ分離する。外部 API 翻訳は adapter 側へ閉じる。

use crate::{
    Result, application,
    domain::{
        commands::{
            EnrollPrimaryCommand, EnrollSpareCommand, GetCommand,
            ProvisionPasswordStoreRemoteCommand, PutCommand, RegisterGpgBackupCommand,
            RestoreGpgCommand, RestorePassCommand, SetupCommand, VerifyYubikeyCommand,
        },
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
                let _ = options;
                application::run_setup_with::run_setup_with(
                    SetupCommand,
                    &mut ports.device,
                    &ports.process_io,
                    &mut ports.storage,
                )
            }
            super::super::YubikeyCommand::Put(options) => {
                let command = PutCommand {
                    name: super::super::parse_secret_name(&options.name)?,
                    force: options.force,
                };
                application::run_put_with_prompt::run_put_with_prompt(
                    command,
                    &mut ports.device,
                    &ports.process_io,
                    &mut ports.storage,
                )
                .await
            }
            super::super::YubikeyCommand::Get(options) => application::run_get_with::run_get_with(
                GetCommand {
                    name: super::super::parse_secret_name(&options.name)?,
                },
                &mut ports.device,
                &ports.process_io,
                &mut ports.storage,
                &ports.process_io,
            ),
            super::super::YubikeyCommand::EnrollPrimary(options) => {
                let _ = options;
                let command = EnrollPrimaryCommand;
                application::run_enroll_primary_with_prompt::run_enroll_primary_with_prompt(
                    command,
                    &mut ports.device,
                    &ports.process_io,
                    &ports.process_io,
                    &mut ports.storage,
                    &ports.report,
                )
                .await
            }
            super::super::YubikeyCommand::EnrollSpare(options) => {
                let _ = options;
                let command = EnrollSpareCommand;
                application::run_enroll_spare_with_prompt::run_enroll_spare_with_prompt(
                    command,
                    &mut ports.device,
                    &ports.process_io,
                    &ports.process_io,
                    &mut ports.storage,
                    &ports.report,
                )
                .await
            }
        },
        super::super::SecretsCommand::VerifyYubikey(options) => {
            application::run_verify_yubikey_with::run_verify_yubikey_with(
                VerifyYubikeyCommand {
                    checks: options
                        .check
                        .into_iter()
                        .map(|check| match check {
                            super::super::VerifyCheck::Vault => ExternalCheck::Vault,
                        })
                        .collect(),
                    all: options.all,
                },
                application::run_verify_yubikey_with::VerifyYubikeyRuntime {
                    device: &mut ports.device,
                    process: &ports.process_io,
                    secret_input: &ports.process_io,
                    storage: &mut ports.storage,
                    report: &ports.report,
                    vault_client: &ports.vault_client,
                    gpg_recipient: &mut ports.gpg_recipient,
                },
            )
            .await
        }
        super::super::SecretsCommand::RestoreGpg(options) => {
            let _ = options;
            application::run_restore_gpg::run_restore_gpg(
                RestoreGpgCommand,
                application::run_restore_gpg::RestoreGpgRuntime {
                    device: &mut ports.device,
                    process: &ports.process_io,
                    secret_input: &ports.process_io,
                    storage: &mut ports.storage,
                    vault_client: &ports.vault_client,
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
            let _ = options;
            application::run_restore_pass::run_restore_pass(
                RestorePassCommand,
                application::run_restore_pass::RestorePassRuntime {
                    device: &mut ports.device,
                    process: &ports.process_io,
                    secret_input: &ports.process_io,
                    storage: &mut ports.storage,
                    vault_client: &ports.vault_client,
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
                let _ = options;
                application::run_register_gpg_backup_primary::run_register_gpg_backup_primary(
                    RegisterGpgBackupCommand,
                    application::run_register_gpg_backup_primary::RegisterGpgBackupPrimaryRuntime {
                        device: &mut ports.device,
                        pin_input: &ports.process_io,
                        secret_input: &ports.process_io,
                        storage: &mut ports.storage,
                        keyring: &mut ports.gpg_keyring,
                        store: &ports.password_store,
                        recipient: &mut ports.gpg_recipient,
                        vault_client: &ports.vault_client,
                    },
                )
                .await
            }
        },
        super::super::SecretsCommand::PassRemote(options) => match options.command {
            super::super::PassRemoteCommand::Register(options) => {
                let _ = options;
                application::run_provision_password_store_remote::run_provision_password_store_remote(
                    ProvisionPasswordStoreRemoteCommand,
                    application::run_provision_password_store_remote::ProvisionPasswordStoreRemoteRuntime {
                        device: &mut ports.device,
                        pin_input: &ports.process_io,
                        secret_input: &ports.process_io,
                        storage: &mut ports.storage,
                        vault_client: &ports.vault_client,
                        store: &ports.password_store,
                        url_input: &ports.process_io,
                    },
                )
                .await
            }
        },
    }
}
