//! `dotfiles secrets` の CLI orchestration 層。
//!
//! この機能は CLI、application、domain、adapter、support の責務に分ける。
//! CLI 入口は clap option の型付けと公開 command 名を固定し、composition root は
//! adapter concrete の所有関係だけを確定する。secret の取得順序や device 操作の
//! 失敗契約は application 以下へ閉じ込める。
//!
//! domain は command 入力、process 保護、実機 discovery に依存しない。保護メモリや
//! 端末 I/O の業務語彙を持たない部品は support として扱い、use case の順序は application に置く。

/// adapter concrete modules を composition root からだけ到達できる範囲に閉じる。
mod adapters {
    mod bw;
    mod gpg;
    mod io;
    mod yubikey;

    pub(in crate::secrets) use bw::BwsClientAdapter;
    pub(in crate::secrets) use gpg::{BackupCipherAdapter, GpgKeyringAdapter, SshAgentAdapter};
    pub(in crate::secrets) use io::{JsonReportAdapter, ProcessIoAdapter};
    pub(in crate::secrets) use yubikey::{
        DeviceSelectionAdapter, GpgRecipientAdapter, StorageAdapter,
    };
}
mod application;
pub mod domain;
mod entrypoint;
pub mod ports;
mod support;

use clap::{Args, Subcommand, ValueEnum};
use domain::piv::SecretName;
use support::protection::SecretSession;

use crate::Result;

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
    RestoreGpg(RestoreGpgOptions),
    GpgBackup(GpgBackupOptions),
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

#[derive(Args)]
/// `gpg-secret-key-backup` envelope を接続中 YubiKey で復号して鍵リングへ復元する option。
struct RestoreGpgOptions {
    #[arg(long)]
    serial: Option<u32>,
}

#[derive(Args)]
/// `gpg-secret-key-backup` の registration / recipient 追加を公開する option。
struct GpgBackupOptions {
    #[command(subcommand)]
    command: GpgBackupCommand,
}

#[derive(Subcommand)]
/// `gpg-secret-key-backup` envelope の primary 登録と spare recipient 追加。
enum GpgBackupCommand {
    Register(GpgBackupRegisterOptions),
    AddSpare(GpgBackupAddSpareOptions),
}

#[derive(Args)]
/// 既存環境の GPG secret key を encrypted envelope 化して primary 登録する option。
struct GpgBackupRegisterOptions {
    #[arg(long)]
    primary_fingerprint: String,
    #[arg(long)]
    serial: Option<u32>,
}

#[derive(Args)]
/// 既存 envelope を復号して spare YubiKey の recipient を追加する option。
struct GpgBackupAddSpareOptions {
    #[arg(long)]
    unwrap_serial: Option<u32>,
    #[arg(long)]
    spare_serial: Option<u32>,
    /// 非対話実行で BWS secret の上書き更新を明示的に許可する。
    #[arg(long)]
    yes: bool,
}

#[derive(Args)]
/// GPG authentication subkey 由来の SSH 公開鍵を扱う最上位 command。
pub(crate) struct GpgOptions {
    #[command(subcommand)]
    command: GpgCommand,
}

#[derive(Subcommand)]
/// GitHub SSH keys 登録向けの GPG SSH 公開鍵出力 command。
enum GpgCommand {
    ExportSshPublicKey(GpgExportSshPublicKeyOptions),
}

#[derive(Args)]
/// authentication subkey 由来の OpenSSH 公開鍵を出力する option。
struct GpgExportSshPublicKeyOptions {
    #[arg(long)]
    primary_fingerprint: String,
}

/// CLI で parse 済みの `dotfiles secrets` command を entrypoint 境界へ渡す。
///
/// CLI 入口は command 定義と option 変換だけを担い、adapter concrete 生成と port 束ねは
/// composition root へ閉じる。
pub(crate) async fn run(options: SecretsOptions) -> Result<()> {
    let _session = SecretSession::start()?;
    let mut ports = RuntimePorts::production();
    entrypoint::run(options, &mut ports).await
}

/// CLI で parse 済みの `dotfiles gpg` command を application use case へ渡す。
///
/// secret material を扱わない公開鍵出力経路であり、composition root は keyring/ssh-output adapter だけを
/// 束ねる。command 定義と option 変換だけをここで行い、鍵リング解決と出力翻訳は adapter へ閉じる。
pub(crate) fn run_gpg(options: GpgOptions) -> Result<()> {
    let mut keyring = adapters::GpgKeyringAdapter::default();
    let output = adapters::ProcessIoAdapter::default();
    match options.command {
        GpgCommand::ExportSshPublicKey(options) => {
            let primary_fingerprint =
                domain::gpg_backup::PrimaryFingerprint::parse(&options.primary_fingerprint)?;
            application::run_export_ssh_public_key::run_export_ssh_public_key(
                domain::commands::ExportSshPublicKeyCommand {
                    primary_fingerprint,
                },
                &mut keyring,
                &output,
            )
        }
    }
}

/// production command path の composition root が所有する実 adapter 群。
pub(in crate::secrets) struct RuntimePorts {
    pub(in crate::secrets) device: adapters::DeviceSelectionAdapter,
    pub(in crate::secrets) spare_device: adapters::DeviceSelectionAdapter,
    pub(in crate::secrets) device_pin_policy: adapters::DeviceSelectionAdapter,
    pub(in crate::secrets) process_io: adapters::ProcessIoAdapter,
    pub(in crate::secrets) storage: adapters::StorageAdapter,
    pub(in crate::secrets) report: adapters::JsonReportAdapter,
    pub(in crate::secrets) bws_client: adapters::BwsClientAdapter,
    pub(in crate::secrets) gpg_recipient: adapters::GpgRecipientAdapter,
    pub(in crate::secrets) backup_cipher: adapters::BackupCipherAdapter,
    pub(in crate::secrets) gpg_keyring: adapters::GpgKeyringAdapter,
    pub(in crate::secrets) ssh_agent: adapters::SshAgentAdapter,
}

impl RuntimePorts {
    /// production 用の adapter concrete 群を構築する。
    fn production() -> Self {
        Self {
            device: adapters::DeviceSelectionAdapter::default(),
            spare_device: adapters::DeviceSelectionAdapter::default(),
            device_pin_policy: adapters::DeviceSelectionAdapter::default(),
            process_io: adapters::ProcessIoAdapter::default(),
            storage: adapters::StorageAdapter::default(),
            report: adapters::JsonReportAdapter::default(),
            bws_client: adapters::BwsClientAdapter,
            gpg_recipient: adapters::GpgRecipientAdapter::default(),
            backup_cipher: adapters::BackupCipherAdapter::default(),
            gpg_keyring: adapters::GpgKeyringAdapter::default(),
            ssh_agent: adapters::SshAgentAdapter::default(),
        }
    }
}

/// CLI 入力は利用者向け kebab-case 名に限定し、wire format の numeric id を露出しない。
fn parse_secret_name(value: &str) -> std::result::Result<SecretName, String> {
    value
        .parse()
        .map_err(|_| format!("unsupported YubiKey secret name: {value}"))
}
