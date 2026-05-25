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
pub mod ports;
mod support;

use clap::{Args, Subcommand, ValueEnum};
use domain::SecretName;
use support::protection::ProtectedSecret;
use zeroize::Zeroizing;

use crate::Result;

/// stdin JSON から読み出した enrollment secret set の raw bytes。
///
/// adapter が JSON を decode して構築し、application 層が `SecretSession` へ移送する。
/// adapter と application の両方から参照するため、secrets module トップレベルに定義する。
/// `SecretsBoundary::read_enrollment_json_bytes` の返り値型として ports module からも参照される。
pub struct EnrollmentBytes {
    pub bw_email: Zeroizing<Vec<u8>>,
    pub bw_password: Zeroizing<Vec<u8>>,
    pub bws_access_token: Zeroizing<Vec<u8>>,
}

/// 登録に必要な 3 field を同じ保護 session で所有する共通型。
///
/// port と application の両方から参照するため、secrets module トップレベルに定義する。
pub(self) struct EnrollmentSecretSet<'session> {
    pub(self) bw_email: ProtectedSecret<'session>,
    pub(self) bw_password: ProtectedSecret<'session>,
    pub(self) bws_access_token: ProtectedSecret<'session>,
}

impl<'session> EnrollmentSecretSet<'session> {
    /// 同じ `SecretSession` に所属する 3 field から登録対象 secret を構築する。
    pub(self) fn new(
        bw_email: ProtectedSecret<'session>,
        bw_password: ProtectedSecret<'session>,
        bws_access_token: ProtectedSecret<'session>,
    ) -> Self {
        Self {
            bw_email,
            bw_password,
            bws_access_token,
        }
    }
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

/// CLI で parse 済みの `dotfiles secrets` command を実機 YubiKey 境界で実行する。
///
/// 実プロセス境界（`RealSecretsBoundary`）の組み立てはここで行い、application 層は
/// port 契約だけを通じて境界を利用する。
pub(crate) fn run(options: SecretsOptions) -> Result<()> {
    let mut boundary = adapters::process_boundary::RealSecretsBoundary;
    application::run_with_boundary(options, &mut boundary)
}

/// CLI 入力は利用者向け kebab-case 名に限定し、wire format の numeric id を露出しない。
fn parse_secret_name(value: &str) -> std::result::Result<SecretName, String> {
    value
        .parse()
        .map_err(|_| format!("unsupported YubiKey secret name: {value}"))
}

/// argv を `dotfiles secrets <subcommand>` として解釈し、与えられた境界で use case を実行する。
///
/// 実プロセスの I/O・device 取得契約は呼び出し側の `SecretsBoundary` 実装が差し替える。
/// tests 層の stub crate（`dotfiles-cli-secrets-test-stub`）が production 経路を駆動するために使う。
pub fn run_with_args<I, T, B>(args: I, boundary: &mut B) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
    B: ports::SecretsBoundary,
{
    use clap::Parser;

    #[derive(Parser)]
    #[command(name = "dotfiles")]
    struct ArgsCli {
        #[command(subcommand)]
        command: ArgsCommand,
    }

    #[derive(clap::Subcommand)]
    enum ArgsCommand {
        Secrets(SecretsOptions),
    }

    let ArgsCommand::Secrets(options) = ArgsCli::try_parse_from(args)?.command;
    application::run_with_boundary(options, boundary)
}
