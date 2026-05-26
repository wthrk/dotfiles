//! `dotfiles secrets` CLI 統合テストの stub binary。
//!
//! production crate（`dotfiles-cli`）と同じ application / port / domain を再利用しつつ、
//! 実機 YubiKey の代わりに in-memory test double device を使って use case を実行する。
//! test double の定義はこの crate に閉じ、production binary の実行経路には含まれない。

mod device;

use std::process::ExitCode;

use anyhow::Context;
use clap::{Parser, Subcommand};

use dotfiles_cli::run_with_args;
use dotfiles_cli_secrets_test_contract::{
    CORRUPT_SECRET_ENV, PRIMARY_STUB_STATE_ENV, READ_PIN_FROM_TTY_ENV, SEED_BW_EMAIL_ENV,
    SEED_BW_PASSWORD_ENV, SEED_BWS_ACCESS_TOKEN_ENV, SPARE_STUB_STATE_ENV, STUB_STATE_ENV,
};

use device::{TestDeviceState, TestStubBoundary};

/// stub binary の最上位 CLI 構造。
///
/// `dotfiles secrets --test-stub-yubikey <subcommand>` の形式で入力を受ける。
#[derive(Parser)]
#[command(name = "dotfiles")]
struct StubCli {
    #[command(subcommand)]
    command: StubTopCommand,
}

#[derive(Subcommand)]
enum StubTopCommand {
    /// `dotfiles secrets` を in-memory stub device で実行する。
    Secrets {
        /// in-memory stub device を使う（統合テスト専用の hidden flag）。
        #[arg(long, hide = true)]
        test_stub_yubikey: bool,
        #[command(subcommand)]
        subcommand: StubSecretsSubcommand,
    },
}

/// `dotfiles secrets` 配下の subcommand を stub binary 向けに定義する。
#[derive(Subcommand)]
enum StubSecretsSubcommand {
    #[command(name = "yubikey")]
    Yubikey {
        #[command(subcommand)]
        command: StubYubikeyCommand,
    },
    #[command(name = "verify-yubikey")]
    VerifyYubikey {
        #[arg(long)]
        serial: Option<u32>,
        #[arg(long)]
        check: Vec<String>,
        #[arg(long)]
        all: bool,
    },
}

#[derive(Subcommand)]
enum StubYubikeyCommand {
    #[command(name = "setup")]
    Setup {
        #[arg(long)]
        serial: Option<u32>,
    },
    #[command(name = "put")]
    Put {
        name: String,
        #[arg(long)]
        serial: Option<u32>,
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        force: bool,
    },
    #[command(name = "get")]
    Get {
        name: String,
        #[arg(long)]
        serial: Option<u32>,
    },
    #[command(name = "enroll-primary")]
    EnrollPrimary {
        #[arg(long)]
        serial: Option<u32>,
        #[arg(long)]
        stdin_json: bool,
    },
    #[command(name = "enroll-spare")]
    EnrollSpare {
        #[arg(long)]
        primary_serial: Option<u32>,
        #[arg(long)]
        spare_serial: Option<u32>,
        #[arg(long)]
        stdin_json: bool,
    },
    #[command(name = "rotate-bws-token")]
    RotateBwsToken {
        #[arg(long)]
        serial: Option<u32>,
        #[arg(long)]
        stdin: bool,
    },
}

/// stub binary の実行本体。
///
/// CLI 引数を parse して stub boundary を組み立て、`dotfiles_cli::testing::run_secrets_cli` で
/// production の application layer を実行する。
fn run() -> anyhow::Result<()> {
    let cli = StubCli::parse();
    let StubTopCommand::Secrets {
        test_stub_yubikey,
        subcommand,
    } = cli.command;

    if !test_stub_yubikey {
        anyhow::bail!("--test-stub-yubikey is required for stub binary");
    }

    let stub_state = std::env::var(STUB_STATE_ENV)
        .ok()
        .map(|v| parse_device_state(&v))
        .transpose()
        .context("invalid STUB_STATE_ENV")?;
    let primary_state = std::env::var(PRIMARY_STUB_STATE_ENV)
        .ok()
        .map(|v| parse_device_state(&v))
        .transpose()
        .context("invalid PRIMARY_STUB_STATE_ENV")?;
    let spare_state = std::env::var(SPARE_STUB_STATE_ENV)
        .ok()
        .map(|v| parse_device_state(&v))
        .transpose()
        .context("invalid SPARE_STUB_STATE_ENV")?;

    let corrupt_secret = std::env::var(CORRUPT_SECRET_ENV).ok();

    let read_pin_from_tty = std::env::var(READ_PIN_FROM_TTY_ENV).as_deref() == Ok("true");

    let seed_bw_email = std::env::var(SEED_BW_EMAIL_ENV).ok();
    let seed_bw_password = std::env::var(SEED_BW_PASSWORD_ENV).ok();
    let seed_bws_access_token = std::env::var(SEED_BWS_ACCESS_TOKEN_ENV).ok();

    let config = device::TestStubConfig {
        stub_state,
        primary_state,
        spare_state,
        corrupt_secret,
        read_pin_from_tty,
        seed_bw_email,
        seed_bw_password,
        seed_bws_access_token,
    };

    let args = build_args_from_subcommand(subcommand);
    let mut boundary = TestStubBoundary::new(config);
    run_with_args(args, &mut boundary)
}

/// stub 向け subcommand を `dotfiles secrets <subcommand>` 形式の argv へ変換する。
fn build_args_from_subcommand(subcommand: StubSecretsSubcommand) -> Vec<String> {
    let mut args = vec!["dotfiles".to_owned(), "secrets".to_owned()];
    match subcommand {
        StubSecretsSubcommand::Yubikey { command } => {
            args.push("yubikey".to_owned());
            match command {
                StubYubikeyCommand::Setup { serial } => {
                    args.push("setup".to_owned());
                    if let Some(s) = serial {
                        args.push("--serial".to_owned());
                        args.push(s.to_string());
                    }
                }
                StubYubikeyCommand::Put {
                    name,
                    serial,
                    stdin,
                    force,
                } => {
                    args.push("put".to_owned());
                    args.push(name.to_string());
                    if let Some(s) = serial {
                        args.push("--serial".to_owned());
                        args.push(s.to_string());
                    }
                    if stdin {
                        args.push("--stdin".to_owned());
                    }
                    if force {
                        args.push("--force".to_owned());
                    }
                }
                StubYubikeyCommand::Get { name, serial } => {
                    args.push("get".to_owned());
                    args.push(name.to_string());
                    if let Some(s) = serial {
                        args.push("--serial".to_owned());
                        args.push(s.to_string());
                    }
                }
                StubYubikeyCommand::EnrollPrimary { serial, stdin_json } => {
                    args.push("enroll-primary".to_owned());
                    if let Some(s) = serial {
                        args.push("--serial".to_owned());
                        args.push(s.to_string());
                    }
                    if stdin_json {
                        args.push("--stdin-json".to_owned());
                    }
                }
                StubYubikeyCommand::EnrollSpare {
                    primary_serial,
                    spare_serial,
                    stdin_json,
                } => {
                    args.push("enroll-spare".to_owned());
                    if let Some(s) = primary_serial {
                        args.push("--primary-serial".to_owned());
                        args.push(s.to_string());
                    }
                    if let Some(s) = spare_serial {
                        args.push("--spare-serial".to_owned());
                        args.push(s.to_string());
                    }
                    if stdin_json {
                        args.push("--stdin-json".to_owned());
                    }
                }
                StubYubikeyCommand::RotateBwsToken { serial, stdin } => {
                    args.push("rotate-bws-token".to_owned());
                    if let Some(s) = serial {
                        args.push("--serial".to_owned());
                        args.push(s.to_string());
                    }
                    if stdin {
                        args.push("--stdin".to_owned());
                    }
                }
            }
        }
        StubSecretsSubcommand::VerifyYubikey { serial, check, all } => {
            args.push("verify-yubikey".to_owned());
            if let Some(s) = serial {
                args.push("--serial".to_owned());
                args.push(s.to_string());
            }
            for c in check {
                args.push("--check".to_owned());
                args.push(c);
            }
            if all {
                args.push("--all".to_owned());
            }
        }
    }
    args
}

fn parse_device_state(value: &str) -> anyhow::Result<TestDeviceState> {
    match value {
        "fresh" => Ok(TestDeviceState::Fresh),
        "initialized" => Ok(TestDeviceState::Initialized),
        "provisioned" => Ok(TestDeviceState::Provisioned),
        "writable-bws-access-token" => Ok(TestDeviceState::WritableBwsAccessToken),
        other => anyhow::bail!("unknown device state: {other}"),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err:#}");
            ExitCode::FAILURE
        }
    }
}
