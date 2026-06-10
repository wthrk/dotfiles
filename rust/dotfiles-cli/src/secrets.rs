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
    mod git;
    mod gpg;
    mod io;
    mod yubikey;

    pub(in crate::secrets) use bw::{BwLoginAdapter, BwsClientAdapter};
    pub(in crate::secrets) use git::{GitCloneAdapter, PasswordStoreAdapter};
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
    RestorePass(RestorePassOptions),
    BwLogin(BwLoginOptions),
    GpgBackup(GpgBackupOptions),
    PassRemote(PassRemoteOptions),
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
    Setup(SetupOptions),
    Put(PutOptions),
    Get(GetOptions),
    EnrollPrimary(EnrollPrimaryOptions),
    EnrollSpare(EnrollSpareOptions),
    RotateBwsToken(RotateBwsTokenOptions),
}

#[derive(Args)]
/// 接続中 YubiKey が 1 本だけであることを確認して storage を初期化する。
struct SetupOptions {}

#[derive(Args)]
/// 1 secret を接続中の単一 YubiKey に保存する低水準 command の option。
struct PutOptions {
    #[arg(value_parser = parse_secret_name)]
    name: SecretName,
    #[arg(long)]
    stdin: bool,
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
/// 1 secret を接続中の単一 YubiKey から取得する低水準 command の option。
struct GetOptions {
    #[arg(value_parser = parse_secret_name)]
    name: SecretName,
}

#[derive(Args)]
/// 接続中の単一 primary YubiKey に bootstrap secret 一式を初期登録する option。
struct EnrollPrimaryOptions {}

#[derive(Args)]
/// 接続中の単一 spare YubiKey に CLI secret input port から受け取る bootstrap secret 一式を登録する option。
struct EnrollSpareOptions {}

#[derive(Args)]
/// `rotate-bws-token` で更新する token の受け取り境界を表す option。
struct RotateBwsTokenOptions {
    #[arg(long)]
    stdin: bool,
}

#[derive(Args)]
/// YubiKey に保存された secret と外部確認項目を検証する option。
///
/// `--check bw-login`（または `--all`）の bw-login 外部確認では、通常 YubiKey の `bw-email` を使う。
/// email override が必要な場合は `--email <email>` を使う（yubikey-secret-storage-design.md の `dotfiles secrets verify-yubikey` 節）。override は `bw-login` の
/// `BwLoginOptions` の `--email` と同じ意味・体裁で、指定時は YubiKey の `bw-email` を読まない。
struct VerifyYubikeyOptions {
    #[arg(long, value_enum)]
    check: Vec<VerifyCheck>,
    #[arg(long)]
    all: bool,
    /// bw-login 外部確認で YubiKey の `bw-email` を使わず、指定した login email で login する override（yubikey-secret-storage-design.md の `dotfiles secrets verify-yubikey` 節）。
    #[arg(long)]
    email: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
/// `verify-yubikey --check` で追加する外部確認項目。
enum VerifyCheck {
    Bws,
    BwLogin,
}

#[derive(Args)]
/// YubiKey から `bw-email` / `bw-password` を取得し Bitwarden Password Manager CLI に login / unlock する option。
///
/// `bw-email` は通常 YubiKey の値を使い、override が必要な場合だけ `--email <email>` を許可する（spec L178）。
/// master password は子プロセスの `BW_PASSWORD` env でだけ渡し、argv には載せない。YubiKey OTP は実行時に
/// 可視入力で受け取り、argv（`--code`）へ載る単回トークンとして扱う。
struct BwLoginOptions {
    /// YubiKey の `bw-email` を使わず、指定した login email で login する override。
    #[arg(long)]
    email: Option<String>,
}

#[derive(Args)]
/// `gpg-secret-key-backup` envelope を接続中 YubiKey で復号して鍵リングへ復元する option。
struct RestoreGpgOptions {}

#[derive(Args)]
/// `password-store-remote` を取得し private `password-store` を SSH clone する option。
struct RestorePassOptions {}

#[derive(Args)]
/// `gpg-secret-key-backup` の事前登録状態確認を公開する option。
struct GpgBackupOptions {
    #[command(subcommand)]
    command: GpgBackupCommand,
}

#[derive(Subcommand)]
/// `gpg-secret-key-backup` envelope の事前登録状態確認。
enum GpgBackupCommand {
    Register(GpgBackupRegisterOptions),
}

#[derive(Args)]
/// 既存 `gpg-secret-key-backup` envelope が接続中 YubiKey と整合するか確認する option。
///
/// 現行 CLI は project 不在なら `dotfiles-secret-recovery` を作成するが、未登録状態から
/// `gpg-secret-key-backup` 自体を新規作成も更新もしない。project 内で secret が未登録なら停止し、
/// 既存 1 件が primary fingerprint・接続中 recipient・2 recipient 以上条件を満たす場合だけ成功する。
/// 初回 envelope 作成の正本手順は別途必要で、この command や provisioning script だけでは
/// 初期プロビジョニング完了にならない。
struct GpgBackupRegisterOptions {}

#[derive(Args)]
/// `password-store-remote` の provisioning（保管側 create/use）を公開する option。
///
/// command 名 `pass-remote` は、`gpg-backup`（`gpg-secret-key-backup` の保管 command 群）と対称に
/// `password-store-remote` secret の保管 command 群を表す。設計「初期登録手順」step3 が定める保管経路
/// （管理 plane の bootstrap）を、復旧本線 command（`restore-pass`）と区別して provisioning 動詞 `register`
/// 配下へ置くため、`restore-pass` ではなく secret 名に揃えた `pass-remote register` を採用する。
struct PassRemoteOptions {
    #[command(subcommand)]
    command: PassRemoteCommand,
}

#[derive(Subcommand)]
/// `password-store-remote` secret の create / use を行う provisioning command 群。
enum PassRemoteCommand {
    Register(PassRemoteRegisterOptions),
}

#[derive(Args)]
/// private `password-store` の clone URL を BWS へ create または既存照合する option。
///
/// clone URL は private repo の SSH clone URL であって秘密情報ではない。configured origin が観測できる場合は
/// その repository identity を優先し、origin が無い場合だけ controlling TTY の可視対話入力へ委譲する。
/// provisioning script は URL を argv / stdin / 環境変数で中継しない。
/// BWS に既存 `password-store-remote` がある場合も、configured origin から期待値を導けるときだけ照合に成功し、
/// origin が無い既存値は fail-closed で停止する。
///
/// この command は YubiKey storage を読まない。BWS 登録に使う access token は hidden prompt（TTY）/
/// pipe（stdin）から保護値として受け取り、YubiKey へ保存しない。YubiKey の `bws-access-token` には
/// 別経路で復旧用の最小権限 token を保存する前提のため、token を argv へ載せる option も設けない。
struct PassRemoteRegisterOptions {}

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
struct GpgExportSshPublicKeyOptions {}

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
    let store = adapters::PasswordStoreAdapter::default();
    let output = adapters::ProcessIoAdapter::default();
    match options.command {
        GpgCommand::ExportSshPublicKey(options) => {
            let _ = options;
            application::run_export_ssh_public_key::run_export_ssh_public_key(
                domain::commands::ExportSshPublicKeyCommand,
                &mut keyring,
                &store,
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
    pub(in crate::secrets) bw_login: adapters::BwLoginAdapter,
    pub(in crate::secrets) gpg_recipient: adapters::GpgRecipientAdapter,
    pub(in crate::secrets) backup_cipher: adapters::BackupCipherAdapter,
    pub(in crate::secrets) gpg_keyring: adapters::GpgKeyringAdapter,
    pub(in crate::secrets) ssh_agent: adapters::SshAgentAdapter,
    pub(in crate::secrets) password_store: adapters::PasswordStoreAdapter,
    pub(in crate::secrets) git_clone: adapters::GitCloneAdapter,
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
            bw_login: adapters::BwLoginAdapter,
            gpg_recipient: adapters::GpgRecipientAdapter::default(),
            backup_cipher: adapters::BackupCipherAdapter::default(),
            gpg_keyring: adapters::GpgKeyringAdapter::default(),
            ssh_agent: adapters::SshAgentAdapter::default(),
            password_store: adapters::PasswordStoreAdapter::default(),
            git_clone: adapters::GitCloneAdapter::default(),
        }
    }
}

/// CLI 入力は利用者向け kebab-case 名に限定し、wire format の numeric id を露出しない。
fn parse_secret_name(value: &str) -> std::result::Result<SecretName, String> {
    value
        .parse()
        .map_err(|_| format!("unsupported YubiKey secret name: {value}"))
}
