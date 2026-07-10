//! `dotfiles secrets` bounded context を提供する crate。
//!
//! この crate は secret-recovery の CLI orchestration から application、domain、
//! adapter、support までの全層を内包する。`dotfiles-cli` はこの crate の公開
//! entrypoint（`run` / `run_gpg`）と clap option（`SecretsOptions` / `GpgOptions`）を
//! 呼ぶ薄い委譲に限定し、secret の取得順序や device 操作の失敗契約はこの crate 内へ閉じる。
//!
//! CLI 入口は clap option の型付けと公開 command 名を固定し、composition root は
//! adapter concrete の所有関係だけを確定する。domain は command 入力、process 保護、
//! 実機 discovery に依存しない。保護メモリや端末 I/O の業務語彙を持たない部品は
//! support として扱い、use case の順序は application に置く。

/// CLI と secret-recovery の各層で共有する結果型。
///
/// 旧 `dotfiles-cli` 内 module 時代の `crate::Result` 参照を crate 分離後も無改変で
/// 通すため、`dotfiles_core::Result` を crate ルートへ再公開する互換 alias。
pub type Result<T> = dotfiles_core::Result<T>;

/// CLI integration test 専用の internal stub 配線契約。
///
/// `secrets-internal-test-stub` feature でのみ compile し、production build には含めない。
#[cfg(feature = "secrets-internal-test-stub")]
pub mod secrets_internal_test_stub_contract;

/// adapter concrete modules を composition root からだけ到達できる範囲に閉じる。
mod adapters {
    mod bw;
    mod git;
    mod gpg;
    mod io;
    mod yubikey;

    pub(crate) use bw::{BwLoginAdapter, BwsClientAdapter};
    pub(crate) use git::{GitCloneAdapter, PasswordStoreAdapter};
    pub(crate) use gpg::{BackupCipherAdapter, GpgKeyringAdapter, SshAgentAdapter};
    pub(crate) use io::{JsonReportAdapter, ProcessIoAdapter};
    pub(crate) use yubikey::{DeviceSelectionAdapter, GpgRecipientAdapter, StorageAdapter};
}
mod application;
pub(crate) mod domain;
mod entrypoint;
pub(crate) mod ports;
mod support;

use clap::{Args, Subcommand, ValueEnum};
use domain::piv::SecretName;
use support::protection::SecretSession;

#[derive(Args)]
/// 復旧用 secret の保存先と検証手段を選ぶ最上位 command。
pub struct SecretsOptions {
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
///
/// `--check bw-login`（または `--all`）の bw-login 外部確認では、通常 YubiKey の `bw-email` を使う。
/// email override が必要な場合は `--email <email>` を使う（yubikey-secret-storage-design.md L286）。override は `bw-login` の
/// `BwLoginOptions` の `--email` と同じ意味・体裁で、指定時は YubiKey の `bw-email` を読まない。
struct VerifyYubikeyOptions {
    #[arg(long)]
    serial: Option<u32>,
    #[arg(long, value_enum)]
    check: Vec<VerifyCheck>,
    #[arg(long)]
    all: bool,
    /// bw-login 外部確認で YubiKey の `bw-email` を使わず、指定した login email で login する override（yubikey-secret-storage-design.md L286）。
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
    #[arg(long)]
    serial: Option<u32>,
    /// YubiKey の `bw-email` を使わず、指定した login email で login する override。
    #[arg(long)]
    email: Option<String>,
}

#[derive(Args)]
/// `gpg-secret-key-backup` envelope を接続中 YubiKey で復号して鍵リングへ復元する option。
struct RestoreGpgOptions {
    #[arg(long)]
    serial: Option<u32>,
}

#[derive(Args)]
/// `password-store-remote` を取得し private `password-store` を SSH clone する option。
struct RestorePassOptions {
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
    primary_fingerprint: Option<String>,
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
/// `password-store-remote` の provisioning（保管側 create/update）を公開する option。
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
/// `password-store-remote` secret の create / update を行う provisioning command 群。
enum PassRemoteCommand {
    Register(PassRemoteRegisterOptions),
}

#[derive(Args)]
/// private `password-store` の clone URL を BWS へ create または update する option。
///
/// clone URL は private repo の SSH clone URL であって秘密情報ではないため、`--url <value>` で argv 指定
/// できる。`--url` 未指定時は可視プロンプト（対話・入力をエコー）または pipe（stdin）から 1 行を読む。
/// `--yes` は非対話実行での上書き更新を明示許可する。
///
/// この command は YubiKey storage を読まない。BWS 登録・更新に使う access token は hidden prompt（TTY）/
/// pipe（stdin）から保護値として受け取り、YubiKey へ保存しない。YubiKey の `bitwarden-client-secret` には
/// 別経路で復旧用の最小権限 token を保存する前提のため、`--serial` option は持たず、token を argv へ
/// 載せる option も設けない。
struct PassRemoteRegisterOptions {
    /// 登録する `password-store-remote` の clone URL（`git@github.com:<owner>/<repo>.git`）。
    /// 非秘匿値のため argv 指定を許可する。未指定時は可視プロンプト / pipe から読む。
    #[arg(long)]
    url: Option<String>,
    /// 非対話実行で BWS secret の上書き更新を明示的に許可する。
    #[arg(long)]
    yes: bool,
}

#[derive(Args)]
/// GPG authentication subkey 由来の SSH 公開鍵を扱う最上位 command。
pub struct GpgOptions {
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
pub async fn run(options: SecretsOptions) -> Result<()> {
    let _session = SecretSession::start()?;
    let mut ports = RuntimePorts::production();
    entrypoint::run(options, &mut ports).await
}

/// CLI で parse 済みの `dotfiles gpg` command を application use case へ渡す。
///
/// secret material を扱わない公開鍵出力経路であり、composition root は keyring/ssh-output adapter だけを
/// 束ねる。command 定義と option 変換だけをここで行い、鍵リング解決と出力翻訳は adapter へ閉じる。
pub fn run_gpg(options: GpgOptions) -> Result<()> {
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
pub(crate) struct RuntimePorts {
    pub(crate) device: adapters::DeviceSelectionAdapter,
    pub(crate) spare_device: adapters::DeviceSelectionAdapter,
    pub(crate) device_pin_policy: adapters::DeviceSelectionAdapter,
    pub(crate) process_io: adapters::ProcessIoAdapter,
    pub(crate) storage: adapters::StorageAdapter,
    pub(crate) report: adapters::JsonReportAdapter,
    pub(crate) bws_client: adapters::BwsClientAdapter,
    pub(crate) bw_login: adapters::BwLoginAdapter,
    pub(crate) gpg_recipient: adapters::GpgRecipientAdapter,
    pub(crate) backup_cipher: adapters::BackupCipherAdapter,
    pub(crate) gpg_keyring: adapters::GpgKeyringAdapter,
    pub(crate) ssh_agent: adapters::SshAgentAdapter,
    pub(crate) password_store: adapters::PasswordStoreAdapter,
    pub(crate) git_clone: adapters::GitCloneAdapter,
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
