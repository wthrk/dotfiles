//! `dotfiles secrets` の CLI orchestration 層。
//!
//! この機能は orchestration、application、input、device、storage、util に分ける。
//! orchestration は clap option、非対話 precondition、保護区間、YubiKey 操作の順序だけを固定する。
//!
//! `storage` は command 入力、process 保護、実機 discovery に依存しない。process
//! 保護や端末 I/O の汎用部品は `util` に置き、use-case 中の保護済み状態は
//! `application` に置く。

mod application;
mod device;
mod input;
mod storage;
#[cfg(feature = "secrets-test-stub")]
mod test_stub;
mod util;

use clap::{Args, Subcommand, ValueEnum};
use storage::SecretName;

use crate::Result;

#[derive(Args)]
/// GPG、pass、Bitwarden 復旧に必要な秘密情報を扱う。
pub(crate) struct SecretsOptions {
    #[cfg(feature = "secrets-test-stub")]
    #[arg(long, hide = true)]
    test_stub_yubikey: bool,
    #[command(subcommand)]
    command: SecretsCommand,
}

#[derive(Subcommand)]
/// `dotfiles secrets` は低水準 YubiKey 操作と利用者向け検証を別の入口として扱う。
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

/// CLI で parse 済みの `dotfiles secrets` command を実行する。
pub(crate) fn run(options: SecretsOptions) -> Result<()> {
    #[cfg(feature = "secrets-test-stub")]
    if options.test_stub_yubikey {
        let mut boundary = test_stub::TestSecretsBoundary::for_options(&options)?;
        return application::run_with_boundary(options, &mut boundary);
    }

    application::run(options)
}

/// CLI は kebab-case 名だけを受け付け、wire format の secret id 変換とは分離する。
fn parse_secret_name(value: &str) -> std::result::Result<SecretName, String> {
    value
        .parse()
        .map_err(|_| format!("unsupported YubiKey secret name: {value}"))
}
