//! `dotfiles secrets` の CLI orchestration 層。
//!
//! この機能は CLI、application、domain、adapter、support の責務に分ける。
//! CLI 入口は clap option の型付けと公開 command 名を固定し、secret の取得順序や
//! device 操作の失敗契約は application 以下へ閉じ込める。
//!
//! domain は command 入力、process 保護、実機 discovery に依存しない。保護メモリや
//! 端末 I/O の業務語彙を持たない部品は support として扱い、use case の順序は application に置く。

mod adapters;
mod application;
pub mod domain;
mod entrypoint;
pub mod ports;
mod support;

use clap::{Args, Subcommand, ValueEnum};
use domain::piv::SecretName;
use entrypoint::EntrypointCommand;

use crate::Result;

/// 実 adapter 群を所有し、use case ごとに必要な port 引数へ分配する配線境界。
///
/// 各 field は責務別 adapter module に閉じた concrete 型であり、entrypoint/application へ
/// adapter catalog を公開しない。application には個別 port trait としてのみ渡す。
struct SecretsRuntimePorts {
    device: adapters::yubikey::DeviceSelectionAdapter,
    spare_device: adapters::yubikey::DeviceSelectionAdapter,
    device_pin_policy: adapters::yubikey::DeviceSelectionAdapter,
    process_io: adapters::io::ProcessIoAdapter,
    storage: adapters::yubikey::StorageAdapter,
    report: adapters::io::JsonReportAdapter,
    bws_client: adapters::bw::BwsClientAdapter,
}

#[derive(Args)]
/// 復旧用 secret の保存先と検証手段を選ぶ最上位 command。
pub(crate) struct SecretsOptions {
    #[command(subcommand)]
    command: SecretsCommand,
}

#[derive(Subcommand)]
/// YubiKey storage の初期化操作と、復旧手順向けの高水準操作を分けて公開する。
enum SecretsCommand {
    Yubikey(YubikeyOptions),
    VerifyYubikey(VerifyYubikeyOptions),
}

#[derive(Args)]
/// YubiKey PIV 領域に bootstrap secret を保存、取得、検証する。
struct YubikeyOptions {
    #[command(subcommand)]
    command: YubikeyCommand,
}

#[derive(Subcommand)]
/// YubiKey PIV storage の高水準 command と低水準 command。
enum YubikeyCommand {
    Setup(SerialOptions),
    Put(PutOptions),
    Get(GetOptions),
    EnrollPrimary(EnrollPrimaryOptions),
    EnrollSpare(EnrollSpareOptions),
    RotateBwsToken(RotateBwsTokenOptions),
}

#[derive(Args)]
/// 非対話実行では secret 入力前に対象 YubiKey を serial で固定する。
struct SerialOptions {
    #[arg(long)]
    serial: Option<u32>,
}

#[derive(Args)]
/// 1 secret を指定した YubiKey に保存する低水準 command の option。
struct PutOptions {
    #[arg(value_parser = parse_secret_name)]
    name: SecretName,
    #[arg(long)]
    serial: Option<u32>,
    #[arg(long)]
    stdin: bool,
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
/// 1 secret を指定した YubiKey から取得する低水準 command の option。
struct GetOptions {
    #[arg(value_parser = parse_secret_name)]
    name: SecretName,
    #[arg(long)]
    serial: Option<u32>,
}

#[derive(Args)]
/// primary YubiKey に bootstrap secret 一式を初期登録する option。
struct EnrollPrimaryOptions {
    #[arg(long)]
    serial: Option<u32>,
    #[arg(long)]
    stdin_json: bool,
}

#[derive(Args)]
/// spare YubiKey に primary 由来の bootstrap secret 一式を登録する option。
struct EnrollSpareOptions {
    #[arg(long)]
    primary_serial: Option<u32>,
    #[arg(long)]
    spare_serial: Option<u32>,
    #[arg(long)]
    stdin_json: bool,
}

#[derive(Args)]
/// `rotate-bws-token` で更新する YubiKey と token の受け取り境界を表す option。
struct RotateBwsTokenOptions {
    #[arg(long)]
    serial: Option<u32>,
    #[arg(long)]
    stdin: bool,
}

#[derive(Args)]
/// YubiKey に保存された secret と外部確認項目を検証する option。
struct VerifyYubikeyOptions {
    #[arg(long)]
    serial: Option<u32>,
    #[arg(long, value_enum)]
    check: Vec<VerifyCheck>,
    #[arg(long)]
    all: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
/// `verify-yubikey --check` で追加する外部確認項目。
enum VerifyCheck {
    Bws,
    BwLogin,
}

/// CLI で parse 済みの `dotfiles secrets` command を entrypoint 境界へ渡す。
///
/// CLI 入口は command 定義と option 変換だけを担う。adapter concrete 生成と port 束ねは
/// entrypoint の外側の起動配線に閉じ、application へは port trait として渡す。
pub(crate) async fn run(options: SecretsOptions) -> Result<()> {
    let command = entrypoint::command_from_options(options)?;
    let _session = support::protection::SecretSession::start()?;
    let mut ports = SecretsRuntimePorts {
        device: adapters::yubikey::DeviceSelectionAdapter::default(),
        spare_device: adapters::yubikey::DeviceSelectionAdapter::default(),
        device_pin_policy: adapters::yubikey::DeviceSelectionAdapter::default(),
        process_io: adapters::io::ProcessIoAdapter::default(),
        storage: adapters::yubikey::StorageAdapter::default(),
        report: adapters::io::JsonReportAdapter::default(),
        bws_client: adapters::bw::BwsClientAdapter,
    };
    dispatch(command, &mut ports).await
}

/// CLI 入力は利用者向け kebab-case 名に限定し、wire format の numeric id を露出しない。
fn parse_secret_name(value: &str) -> std::result::Result<SecretName, String> {
    value
        .parse()
        .map_err(|_| format!("unsupported YubiKey secret name: {value}"))
}

/// entrypoint で選択済みの command を application use case へ委譲する。
///
/// この配線は adapter concrete を起動境界でだけ所有し、application には port trait として渡す。
/// entrypoint は CLI 入力変換に限定し、adapter/support の concrete 型を知らない。
async fn dispatch(command: EntrypointCommand, ports: &mut SecretsRuntimePorts) -> Result<()> {
    match command {
        EntrypointCommand::Setup(command) => application::run_setup_with::run_setup_with(
            command,
            &mut ports.device,
            &mut ports.storage,
        ),
        EntrypointCommand::PutPrompt(command) => {
            application::run_put_with_prompt::run_put_with_prompt(
                command,
                &mut ports.device,
                &ports.process_io,
                &mut ports.storage,
            )
        }
        EntrypointCommand::PutStdin(command) => {
            application::run_put_with_stdin::run_put_with_stdin(
                command,
                &ports.process_io,
                &mut ports.storage,
            )
        }
        EntrypointCommand::Get(command) => application::run_get_with::run_get_with(
            command,
            &mut ports.device,
            &mut ports.device_pin_policy,
            &ports.process_io,
            &mut ports.storage,
            &ports.process_io,
        ),
        EntrypointCommand::EnrollPrimaryPrompt(command) => {
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
        EntrypointCommand::EnrollPrimaryStdinJson(command) => {
            application::run_enroll_primary_with_stdin_json::run_enroll_primary_with_stdin_json(
                command,
                &mut ports.device,
                &mut ports.device_pin_policy,
                &ports.process_io,
                &ports.process_io,
                &mut ports.storage,
                &ports.report,
            )
        }
        EntrypointCommand::EnrollSparePrompt(command) => {
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
        EntrypointCommand::EnrollSpareStdinJson(command) => {
            application::run_enroll_spare_with_stdin_json::run_enroll_spare_with_stdin_json(
                command,
                &mut ports.spare_device,
                &mut ports.device_pin_policy,
                &ports.process_io,
                &ports.process_io,
                &mut ports.storage,
                &ports.report,
            )
        }
        EntrypointCommand::RotateBwsTokenPrompt(command) => {
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
        EntrypointCommand::RotateBwsTokenStdin(command) => {
            application::run_rotate_bws_token_with_stdin::run_rotate_bws_token_with_stdin(
                command,
                &mut ports.device_pin_policy,
                &ports.process_io,
                &ports.process_io,
                &mut ports.storage,
                &ports.report,
            )
        }
        EntrypointCommand::VerifyYubikey(command) => {
            application::run_verify_yubikey_with::run_verify_yubikey_with(
                command,
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
