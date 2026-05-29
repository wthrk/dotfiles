//! `dotfiles secrets` の entrypoint 配線境界。
//!
//! CLI command 定義で parse 済みの入力を application use case へ橋渡しし、起動時に
//! 必要な adapter 所有関係を確定する。domain rule と外部 API 翻訳は持たない。

use crate::{
    Result,
    secrets::{
        adapters, application,
        domain::values::{
            EnrollPrimaryCommand, EnrollSpareCommand, ExternalCheck, GetCommand, PutCommand,
            RotateBwsTokenCommand, SetupCommand, VerifyYubikeyCommand,
        },
        support::protection::SecretSession,
    },
};

/// 実 adapter 群を所有し、use case ごとに必要な port 引数へ分配する entrypoint 配線。
///
/// 各 field は単一外部機能の adapter であり、この型は起動時の所有と委譲だけを担う。
/// port trait を実装する集約 boundary にはせず、application には個別 port を渡す。
struct EntrypointPorts {
    device: adapters::DeviceSelectionAdapter,
    spare_device: adapters::DeviceSelectionAdapter,
    device_pin_policy: adapters::DeviceSelectionAdapter,
    process_io: adapters::ProcessIoAdapter,
    storage: adapters::StorageAdapter,
    report: adapters::JsonReportAdapter,
    bws_client: adapters::BwsClientAdapter,
}

/// 実 adapter を生成し、parse 済み command を application use case へ渡す。
pub(super) async fn run(options: super::SecretsOptions) -> Result<()> {
    let _session = SecretSession::start()?;
    let mut ports = EntrypointPorts {
        device: adapters::DeviceSelectionAdapter::default(),
        spare_device: adapters::DeviceSelectionAdapter::default(),
        device_pin_policy: adapters::DeviceSelectionAdapter::default(),
        process_io: adapters::ProcessIoAdapter::default(),
        storage: adapters::StorageAdapter::default(),
        report: adapters::JsonReportAdapter::default(),
        bws_client: adapters::BwsClientAdapter::default(),
    };
    dispatch(options, &mut ports).await
}

/// parse 済み command を use case に橋渡しする。
///
/// command ごとの application 呼び出し順序と adapter 所有を entrypoint に閉じ、adapter 層
/// には use case 手順を持ち込まない。
async fn dispatch(options: super::SecretsOptions, ports: &mut EntrypointPorts) -> Result<()> {
    match options.command {
        super::SecretsCommand::Yubikey(options) => match options.command {
            super::YubikeyCommand::Setup(options) => application::run_setup_with::run_setup_with(
                SetupCommand {
                    serial: options.serial,
                },
                &mut ports.device,
                &mut ports.storage,
            ),
            super::YubikeyCommand::Put(options) => {
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
            super::YubikeyCommand::Get(options) => application::run_get_with::run_get_with(
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
            super::YubikeyCommand::EnrollPrimary(options) => {
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
            super::YubikeyCommand::EnrollSpare(options) => {
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
            super::YubikeyCommand::RotateBwsToken(options) => {
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
        super::SecretsCommand::VerifyYubikey(options) => {
            application::run_verify_yubikey_with::run_verify_yubikey_with(
                VerifyYubikeyCommand {
                    serial: options.serial,
                    checks: options
                        .check
                        .into_iter()
                        .map(|check| match check {
                            super::VerifyCheck::Bws => ExternalCheck::Bws,
                            super::VerifyCheck::BwLogin => ExternalCheck::BwLogin,
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
