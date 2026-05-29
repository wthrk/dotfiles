//! `dotfiles secrets` の entrypoint 変換境界。
//!
//! CLI command 定義で parse 済みの入力を application/domain が扱う command value へ変換する。
//! adapter concrete 生成や secret 保護 session 開始は、この entrypoint 境界の外で行う。

use crate::{
    Result,
    secrets::domain::command::{
        EnrollPrimaryCommand, EnrollSpareCommand, ExternalCheck, GetCommand, PutCommand,
        RotateBwsTokenCommand, SetupCommand, VerifyYubikeyCommand,
    },
};

/// CLI subcommand から選択された application use case と domain command value。
///
/// この enum は CLI 入力形式の分岐結果だけを保持する。実 adapter 所有や port 注入は
/// caller 側の配線責務とし、entrypoint は concrete backend を知らない。
pub(super) enum EntrypointCommand {
    Setup(SetupCommand),
    PutPrompt(PutCommand),
    PutStdin(PutCommand),
    Get(GetCommand),
    EnrollPrimaryPrompt(EnrollPrimaryCommand),
    EnrollPrimaryStdinJson(EnrollPrimaryCommand),
    EnrollSparePrompt(EnrollSpareCommand),
    EnrollSpareStdinJson(EnrollSpareCommand),
    RotateBwsTokenPrompt(RotateBwsTokenCommand),
    RotateBwsTokenStdin(RotateBwsTokenCommand),
    VerifyYubikey(VerifyYubikeyCommand),
}

/// parse 済み CLI command を application/domain 境界の command value へ変換する。
///
/// CLI 固有の入力 mode 分岐をここで domain command と use case 選択へ畳み込み、caller は
/// 返却された command を port 注入済み application use case へ委譲する。
pub(super) fn command_from_options(options: super::SecretsOptions) -> Result<EntrypointCommand> {
    let command = match options.command {
        super::SecretsCommand::Yubikey(options) => match options.command {
            super::YubikeyCommand::Setup(options) => EntrypointCommand::Setup(SetupCommand {
                serial: options.serial,
            }),
            super::YubikeyCommand::Put(options) => {
                let command = PutCommand {
                    name: options.name,
                    serial: options.serial,
                    force: options.force,
                };
                if options.stdin {
                    EntrypointCommand::PutStdin(command)
                } else {
                    EntrypointCommand::PutPrompt(command)
                }
            }
            super::YubikeyCommand::Get(options) => EntrypointCommand::Get(GetCommand {
                name: options.name,
                serial: options.serial,
            }),
            super::YubikeyCommand::EnrollPrimary(options) => {
                let command = EnrollPrimaryCommand {
                    serial: options.serial,
                };
                if options.stdin_json {
                    EntrypointCommand::EnrollPrimaryStdinJson(command)
                } else {
                    EntrypointCommand::EnrollPrimaryPrompt(command)
                }
            }
            super::YubikeyCommand::EnrollSpare(options) => {
                let command = EnrollSpareCommand {
                    primary_serial: options.primary_serial,
                    spare_serial: options.spare_serial,
                };
                if options.stdin_json {
                    EntrypointCommand::EnrollSpareStdinJson(command)
                } else {
                    EntrypointCommand::EnrollSparePrompt(command)
                }
            }
            super::YubikeyCommand::RotateBwsToken(options) => {
                let command = RotateBwsTokenCommand {
                    serial: options.serial,
                };
                if options.stdin {
                    EntrypointCommand::RotateBwsTokenStdin(command)
                } else {
                    EntrypointCommand::RotateBwsTokenPrompt(command)
                }
            }
        },
        super::SecretsCommand::VerifyYubikey(options) => {
            EntrypointCommand::VerifyYubikey(VerifyYubikeyCommand {
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
            })
        }
    };
    Ok(command)
}
