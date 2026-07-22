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

/// `yubikey status` が予約 storage の観測済み不整合を検出したときの終了コード。
///
/// 低水準 command の互換的な公開終了コードである。provisioning script はこの値を clear または
/// 他の state transition の根拠にせず、`provision-bws-token` 一回へ遷移全体を委譲する。
/// USB/PCSC/device discovery などの観測失敗はこのコードに変換しない。
pub const SECRET_STORAGE_STATUS_INVALID_EXIT_CODE: u8 = 42;

/// `yubikey put` が完全に未初期化の予約領域を観測したときの終了コード。
///
/// 低水準 command の互換的な公開終了コードである。provisioning script はこの値を `setup`、
/// 再試行、または他の state transition の根拠にしない。
pub const SECRET_STORAGE_UNINITIALIZED_EXIT_CODE: u8 = 43;

/// error chain に `status` が観測した予約 storage 不整合が含まれるかを返す。
///
/// CLI process boundary だけがこれを終了コードへ変換する。外部 I/O 失敗を文言で分類しない。
pub fn is_secret_storage_status_invalid(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.is::<domain::storage::SecretStorageStatusInvalid>())
}

/// error chain に `put` が観測した完全未初期化状態が含まれるかを返す。
pub fn is_secret_storage_uninitialized(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.is::<domain::storage::SecretStorageUninitialized>())
}

/// CLI integration test 専用の internal stub 配線契約。
///
/// `secrets-internal-test-stub` feature でのみ compile し、production build には含めない。
#[cfg(feature = "secrets-internal-test-stub")]
pub mod secrets_internal_test_stub_contract;

// Adapter source はすべて、support-owned concrete backend に対する port trait implementation
// だけを持つ。nested adapter module は state / helper の置き場になってしまうため使わず、
// composition root が各 trait-implementation source を直接 compile する。
#[path = "adapters/bw.rs"]
mod adapter_bw;
#[cfg(feature = "secrets-internal-test-stub")]
#[path = "adapters/bw/internal_stub.rs"]
mod adapter_bw_internal_stub;
#[path = "adapters/git.rs"]
mod adapter_git;
#[cfg(feature = "secrets-internal-test-stub")]
#[path = "adapters/git/internal_stub.rs"]
mod adapter_git_internal_stub;
#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
#[path = "adapters/gpg/cipher_adapter.rs"]
mod adapter_gpg_cipher;
#[cfg(feature = "secrets-internal-test-stub")]
#[path = "adapters/gpg/internal_stub.rs"]
mod adapter_gpg_internal_stub;
#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
#[path = "adapters/gpg/keyring_adapter.rs"]
mod adapter_gpg_keyring;
#[cfg(all(feature = "gpg-backend", not(feature = "secrets-internal-test-stub")))]
#[path = "adapters/gpg/ssh_agent_adapter.rs"]
mod adapter_gpg_ssh_agent;
#[path = "adapters/io.rs"]
mod adapter_io;
#[path = "adapters/yubikey.rs"]
mod adapter_yubikey;
mod application;
mod composition;
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
    Clear(ClearOptions),
    Put(PutOptions),
    Status(SerialOptions),
    EnrollPrimary(EnrollPrimaryOptions),
    EnrollSpare(EnrollSpareOptions),
    RotateBwsToken(RotateBwsTokenOptions),
    /// BWS token storage の観測・必要時修復・保存・復号検証を単一 PIV session で行う。
    ///
    /// PIN-protected management key の同一-session flow は [Yubico PIV PIN-only mode]
    /// (https://docs.yubico.com/yesdk/users-manual/application-piv/pin-only.html#pin-protected)、
    /// repository の error / ykman source evidence は
    /// [`external-sdk-evidence.md`](../../../docs/secret-recovery/external-sdk-evidence.md) を参照する。
    ProvisionBwsToken(ProvisionBwsTokenOptions),
}

#[derive(Args)]
/// 対象 YubiKey を serial で明示指定する共通 option。
struct SerialOptions {
    #[arg(long)]
    serial: Option<u32>,
}

#[derive(Args)]
/// source provisioning の BWS token storage を単一 PIV session で確認・必要時保存・検証する option。
///
/// `--debug` は通常出力を変えず、通常 flow の fixed non-secret phase、解決済み serial、opaque result
/// だけを stderr へ出す。PIV VERIFY failure は raw card status を復元・分類せず、`opaque-error` と
/// 表示する。PIN/token/secret、長さ、hash、raw APDU/status は出さない。repository の one input /
/// one physical VERIFY 契約は
/// [`secret-handling.md`](../../../docs/secret-recovery/secret-handling.md#tty-secret-input)、
/// PIV PIN-only の同一-session flow は
/// [Yubico PIV PIN-only mode](https://docs.yubico.com/yesdk/users-manual/application-piv/pin-only.html#pin-protected)
/// を根拠とする。
struct ProvisionBwsTokenOptions {
    #[arg(long)]
    serial: Option<u32>,
    #[arg(long)]
    debug: bool,
}

#[derive(Args)]
/// 予約済み secret storage 領域を明示確認の上で clear する option。
struct ClearOptions {
    #[arg(long)]
    serial: Option<u32>,
    #[arg(long)]
    yes: bool,
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
/// `verify-yubikey` は無対話の BWS recovery prerequisite だけを確認する。Bitwarden Password Manager login
/// と OTP 入力は復旧フローに不要であり、この command の option / check に含めない。
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
/// BWS 登録・更新に使う access token は YubiKey storage の `bitwarden-client-secret` から読み出す。
/// token を argv / prompt / stdin で受け取る option は設けない。serial 未指定時は単一接続だけを自動解決し、
/// 複数接続では fail-closed する。
struct PassRemoteRegisterOptions {
    /// BWS access token を読む YubiKey serial。未指定時は単一接続 YubiKey を使う。
    #[arg(long)]
    serial: Option<u32>,
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
    let mut runtime = composition::SecretsRuntime::production();
    entrypoint::run(options, &mut runtime).await
}

/// CLI で parse 済みの `dotfiles gpg` command を application use case へ渡す。
///
/// secret material を扱わない公開鍵出力経路である。concrete keyring/output backend は composition が
/// 生成・所有し、この公開入口は command を domain 値へ変換して application use case へ渡す。鍵リング
/// 解決と出力の技術操作は port 実装へ閉じる。
pub fn run_gpg(options: GpgOptions) -> Result<()> {
    let mut runtime = composition::GpgRuntime::production();
    let (keyring, output) = runtime.ports();
    match options.command {
        GpgCommand::ExportSshPublicKey(options) => {
            let primary_fingerprint =
                domain::gpg_backup::PrimaryFingerprint::parse(&options.primary_fingerprint)?;
            application::run_export_ssh_public_key::run_export_ssh_public_key(
                domain::commands::ExportSshPublicKeyCommand {
                    primary_fingerprint,
                },
                keyring,
                output,
            )
        }
    }
}

/// CLI 入力は利用者向け kebab-case 名に限定し、wire format の numeric id を露出しない。
fn parse_secret_name(value: &str) -> std::result::Result<SecretName, String> {
    value
        .parse()
        .map_err(|_| format!("unsupported YubiKey secret name: {value}"))
}
