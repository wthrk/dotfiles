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

    pub(in crate::secrets) use bw::VaultClientAdapter;
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
}

#[derive(Args)]
/// 接続中 YubiKey が 1 本だけであることを確認して storage を初期化する。
struct SetupOptions {}

#[derive(Args)]
/// 1 secret を接続中の単一 YubiKey に保存する低水準 command の option。
struct PutOptions {
    name: String,
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
/// 1 secret を接続中の単一 YubiKey から取得する低水準 command の option。
struct GetOptions {
    name: String,
}

#[derive(Args)]
/// 接続中の単一 primary YubiKey に bootstrap secret 一式を初期登録する option。
struct EnrollPrimaryOptions {}

#[derive(Args)]
/// 接続中の単一 spare YubiKey に CLI secret input port から受け取る bootstrap secret 一式を登録する option。
struct EnrollSpareOptions {}

#[derive(Args)]
/// YubiKey に保存された secret と個人 vault 外部確認項目を検証する option。
struct VerifyYubikeyOptions {
    #[arg(long, value_enum)]
    check: Vec<VerifyCheck>,
    #[arg(long)]
    all: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
/// `verify-yubikey --check` で追加する外部確認項目。
enum VerifyCheck {
    Vault,
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
/// YubiKey storage に保存済みの Bitwarden account API key を使い、Bitwarden 個人 vault の
/// SDK/API adapter 境界で既存 `gpg-secret-key-backup` item を照合する。
/// 新規 1 recipient envelope は作成せず、2 recipient 以上を満たす
/// 既存 secret だけを成功扱いにする。
struct GpgBackupRegisterOptions {}

#[derive(Args)]
/// `password-store-remote` の provisioning（保管側 create/use）を公開する option。
///
/// command 名 `pass-remote` は、`gpg-backup`（`gpg-secret-key-backup` の保管 command 群）と対称に
/// `password-store-remote` secret の保管 command 群を表す。`secret-recovery-spec.md` と
/// `bitwarden-personal-vault-design.md` が定める保管経路を、復旧本線 command（`restore-pass`）と区別して provisioning 動詞 `register`
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
/// private `password-store` の clone URL を Bitwarden 個人 vault へ create または既存照合する option。
///
/// YubiKey storage に保存済みの Bitwarden account API key と、CLI/app input port で取得した master password を使い、
/// SDK/API adapter 境界で `password-store-remote` item を create または既存照合する。登録値は
/// `git@github.com:<owner>/<repo>.git` 形式へ正規化した SSH clone URL を使い、URL は
/// argv/stdin/env ではなく CLI/app 側の input port で取得する。
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
    pub(in crate::secrets) vault_client: adapters::VaultClientAdapter,
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
            vault_client: adapters::VaultClientAdapter,
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
fn parse_secret_name(value: &str) -> crate::Result<SecretName> {
    value.parse().map_err(anyhow::Error::msg)
}
