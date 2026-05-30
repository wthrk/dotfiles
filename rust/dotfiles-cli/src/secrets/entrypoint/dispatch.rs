//! parse 済み CLI command を application use case へ橋渡しする dispatch 境界。
//!
//! command ごとの application 呼び出し順序だけを担い、adapter 生成と support session は
//! composition / runtime 境界へ分離する。外部 API 翻訳は adapter 側へ閉じる。

use crate::{
    Result,
    secrets::{
        application,
        domain::{
            commands::{
                EnrollPrimaryCommand, EnrollSpareCommand, GetCommand, PutCommand,
                RotateBwsTokenCommand, SetupCommand, VerifyYubikeyCommand,
            },
            verification::ExternalCheck,
        },
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
            super::super::YubikeyCommand::Put(options) => {
                let command = PutCommand {
                    name: options.name,
                    serial: options.serial,
                    force: options.force,
                };
                if options.stdin {
                    application::run_put_with_stdin::run_put_with_stdin(
                        command,
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
            super::super::YubikeyCommand::Get(options) => application::run_get_with::run_get_with(
                GetCommand {
                    name: options.name,
                    serial: options.serial,
                },
                &mut ports.device,
                &mut ports.device_pin_policy,
                &ports.process_io,
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
                        &mut ports.device_pin_policy,
                        &ports.process_io,
                        &ports.process_io,
                        &mut ports.storage,
                        &ports.report,
                    )
                } else {
                    application::run_enroll_primary_with_prompt::run_enroll_primary_with_prompt(
                        command,
                        &mut ports.device,
                        &mut ports.device_pin_policy,
                        &ports.process_io,
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
                        &mut ports.spare_device,
                        &mut ports.device_pin_policy,
                        &ports.process_io,
                        &ports.process_io,
                        &mut ports.storage,
                        &ports.report,
                    )
                } else {
                    application::run_enroll_spare_with_prompt::run_enroll_spare_with_prompt(
                        command,
                        &mut ports.device,
                        &mut ports.spare_device,
                        &mut ports.device_pin_policy,
                        &ports.process_io,
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
                        &mut ports.device_pin_policy,
                        &ports.process_io,
                        &ports.process_io,
                        &mut ports.storage,
                        &ports.report,
                    )
                } else {
                    application::run_rotate_bws_token_with_prompt::run_rotate_bws_token_with_prompt(
                        command,
                        &mut ports.device,
                        &mut ports.device_pin_policy,
                        &ports.process_io,
                        &ports.process_io,
                        &ports.process_io,
                        &mut ports.storage,
                        &ports.report,
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
                },
                &mut ports.device,
                &mut ports.device_pin_policy,
                &ports.process_io,
                &mut ports.storage,
                &ports.report,
                &ports.bws_client,
            )
            .await
        }
    }
}
