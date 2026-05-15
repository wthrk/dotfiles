//! `dotfiles secrets` が YubiKey bootstrap secret を登録、取得、検証する処理。
//!
//! secret 本文は引数やログに出さず、prompt、stdin、YubiKey PIV operation の間だけ
//! zeroize 可能な buffer に保持する。

mod device;
mod input;
mod memory;
mod oaep;
mod storage;

use std::{
    collections::BTreeSet,
    io::{self, IsTerminal},
};

use anyhow::bail;
use clap::{Args, Subcommand, ValueEnum};
use device::{open_device, open_spare_device};
use input::{
    prompt_yes_no, protect_zeroizing_secret, read_bootstrap_secrets, read_secret_for_put,
    write_secret_to_stdout,
};
use memory::{InterruptGuard, SecretMemoryGuard};
use storage::{BootstrapSecrets, SecretDevice, SecretName, YubikeyRole};

use crate::Result;

#[derive(Args)]
/// GPG、pass、Bitwarden 復旧に必要な秘密情報を扱う。
pub(crate) struct SecretsOptions {
    #[command(subcommand)]
    command: SecretsCommand,
}

#[derive(Subcommand)]
/// `dotfiles secrets` 配下の操作種別。
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
/// 単一 YubiKey を serial で指定する option。
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

/// CLI で parse 済みの `dotfiles secrets` command を実行する。
pub(crate) fn run(options: SecretsOptions) -> Result<()> {
    match options.command {
        SecretsCommand::Yubikey(options) => run_yubikey(options),
        SecretsCommand::VerifyYubikey(options) => run_verify_yubikey(options),
    }
}

/// `dotfiles secrets yubikey` 配下の command を実行する。
///
/// 低水準 command は単一 secret または storage setup だけを扱い、高水準 command は
/// primary / spare 登録と local verify までを一連の操作として扱う。
fn run_yubikey(options: YubikeyOptions) -> Result<()> {
    match options.command {
        YubikeyCommand::Setup(options) => {
            let mut device = open_device(options.serial)?;
            storage::setup(&mut device)
        }
        YubikeyCommand::Put(options) => {
            let mut device = open_device(options.serial)?;
            storage::check_put_preconditions(&mut device, options.name, options.force)?;
            let secret = read_secret_for_put(options.name, options.stdin)?;
            storage::put(&mut device, options.name, &secret, options.force)
        }
        YubikeyCommand::Get(options) => {
            let mut device = open_device(options.serial)?;
            let secret = storage::get(&mut device, options.name)?;
            write_secret_to_stdout(&secret)?;
            Ok(())
        }
        YubikeyCommand::EnrollPrimary(options) => {
            let mut device = open_device(options.serial)?;
            let secrets = read_bootstrap_secrets(options.stdin_json, None)?;
            let summary = storage::enroll(&mut device, YubikeyRole::Primary, &secrets)?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
            Ok(())
        }
        YubikeyCommand::EnrollSpare(options) => run_enroll_spare(options),
        YubikeyCommand::RotateBwsToken(options) => run_rotate_bws_token(options),
    }
}

/// primary から secret 一式を読み出し、spare に再暗号化して登録する。
///
/// 通常実行では primary YubiKey から 3 secret を復号し終えた後に spare 選択へ進む。
/// `--stdin-json` は primary が使えない migration / recovery 用で、この場合だけ
/// stdin の secret 一式を正本として扱う。
fn run_enroll_spare(options: EnrollSpareOptions) -> Result<()> {
    let interrupt_guard = InterruptGuard::install()?;
    let mut memory = SecretMemoryGuard::prepare()?;
    let (bootstrap, primary_serial, spare) = if options.stdin_json {
        if options.spare_serial.is_none() && !io::stdin().is_terminal() {
            bail!("pass --spare-serial in non-interactive use");
        }
        let spare = if options.spare_serial.is_some() {
            Some(open_spare_device(
                options.spare_serial,
                options.primary_serial,
                &interrupt_guard,
            )?)
        } else {
            None
        };
        if interrupt_guard.interrupted() {
            bail!("interrupted while handling bootstrap secrets");
        }
        (
            read_bootstrap_secrets(true, Some(&mut memory))?,
            options.primary_serial,
            spare,
        )
    } else {
        let mut primary = open_device(options.primary_serial)?;
        let primary_serial = primary.serial();
        let secrets =
            BootstrapSecrets {
                bw_email: memory.lock_secret(
                    SecretName::BwEmail,
                    protect_zeroizing_secret(interrupt_guard.run_yubikey_operation(|| {
                        storage::get(&mut primary, SecretName::BwEmail)
                    })?),
                )?,
                bw_password: memory.lock_secret(
                    SecretName::BwPassword,
                    protect_zeroizing_secret(interrupt_guard.run_yubikey_operation(|| {
                        storage::get(&mut primary, SecretName::BwPassword)
                    })?),
                )?,
                bws_access_token: memory.lock_secret(
                    SecretName::BwsAccessToken,
                    protect_zeroizing_secret(interrupt_guard.run_yubikey_operation(|| {
                        storage::get(&mut primary, SecretName::BwsAccessToken)
                    })?),
                )?,
            };
        (secrets, Some(primary_serial), None)
    };

    let mut spare = match spare {
        Some(spare) => spare,
        None => open_spare_device(options.spare_serial, primary_serial, &interrupt_guard)?,
    };

    let summary = interrupt_guard
        .run_yubikey_operation(|| storage::enroll(&mut spare, YubikeyRole::Spare, &bootstrap))?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

/// BWS token を 1 本または対話で選んだ複数本の YubiKey に保存する。
///
/// 非対話実行では `--serial` で 1 本だけを更新する。対話実行で serial 指定がない場合は
/// token を一度だけ受け取り、利用者が終了を選ぶまで YubiKey 選択と更新を繰り返す。
fn run_rotate_bws_token(options: RotateBwsTokenOptions) -> Result<()> {
    if let Some(serial) = options.serial {
        let mut device = open_device(Some(serial))?;
        storage::check_rotate_preconditions(&mut device)?;
        let token = read_secret_for_put(SecretName::BwsAccessToken, options.stdin)?;
        let summary = storage::rotate_bws_token(&mut device, &token)?;
        println!("{}", serde_json::to_string_pretty(&summary)?);
        return Ok(());
    }

    let mut device = open_device(None)?;
    storage::check_rotate_preconditions(&mut device)?;
    let token = read_secret_for_put(SecretName::BwsAccessToken, options.stdin)?;
    let mut updated_serials = BTreeSet::from([device.serial()]);
    let mut summaries = vec![storage::rotate_bws_token(&mut device, &token)?];

    while prompt_yes_no("Update another YubiKey? [y/N] ")? {
        let mut device = open_device(None)?;
        if !updated_serials.insert(device.serial()) {
            bail!("selected YubiKey was already updated");
        }
        storage::check_rotate_preconditions(&mut device)?;
        summaries.push(storage::rotate_bws_token(&mut device, &token)?);
    }

    println!("{}", serde_json::to_string_pretty(&summaries)?);
    Ok(())
}

/// local storage の復号確認を行い、外部確認要求は利用不可として拒否する。
///
/// 引数なしの実行だけが YubiKey 上の bootstrap secret 復号を検証する。
fn run_verify_yubikey(options: VerifyYubikeyOptions) -> Result<()> {
    if options.all && !options.check.is_empty() {
        bail!("--all and --check cannot be used together");
    }
    if options.all {
        bail!("verify-yubikey --all includes unsupported external checks: bws, bw-login");
    }
    if !options.check.is_empty() {
        let requested = options
            .check
            .iter()
            .map(|check| match check {
                VerifyCheck::Bws => "bws",
                VerifyCheck::BwLogin => "bw-login",
            })
            .collect::<Vec<_>>()
            .join(", ");
        bail!("unsupported external checks requested: {requested}");
    }

    let mut device = open_device(options.serial)?;
    let summary = storage::verify_local_storage(&mut device)?;

    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

/// CLI 引数の secret 名を storage model の closed set に変換する。
fn parse_secret_name(value: &str) -> std::result::Result<SecretName, String> {
    serde_json::from_value(serde_json::Value::String(value.to_owned()))
        .map_err(|_| format!("unsupported YubiKey secret name: {value}"))
}
