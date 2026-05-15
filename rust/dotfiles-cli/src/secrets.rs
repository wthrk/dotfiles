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
mod util;

use std::collections::BTreeSet;

use anyhow::{Context, bail};
use application::{ProtectedBootstrapSecrets, ProtectedSecret, protect_secret_input};
use clap::{Args, Subcommand, ValueEnum};
use device::{open_device, open_spare_device};
use input::{
    parse_bootstrap_secrets_json, read_hidden_secret, read_one_stdin_secret,
    read_visible_secret_line, read_yubikey_pin, write_secret_to_stdout,
};
use storage::{BootstrapSecrets, SecretDevice, SecretName, YubikeyRole};
use util::{
    protection::{InterruptGuard, ProtectedInputBuffer, SecretMemoryGuard},
    terminal::{
        SPARE_SERIAL_NONINTERACTIVE_ERROR, prompt_yes_no,
        stdin_is_terminal as input_stdin_is_terminal,
    },
};
use zeroize::Zeroizing;

use crate::Result;

const MAX_BOOTSTRAP_JSON_LEN: usize = 64 * 1024;
const MAX_SINGLE_STDIN_SECRET_LEN: usize = 16 * 1024;

#[derive(Args)]
/// GPG、pass、Bitwarden 復旧に必要な秘密情報を扱う。
pub(crate) struct SecretsOptions {
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
            require_stdin_secret_source(options.stdin, StdinSecretMode::Single)?;
            let interrupt_guard = InterruptGuard::install()?;
            let memory = SecretMemoryGuard::prepare()?;
            let mut device = open_device(options.serial)?;
            interrupt_guard.run_yubikey_operation(|| {
                storage::check_put_preconditions(&mut device, options.name, options.force)
            })?;
            let secret = read_protected_secret_for_put(options.name, options.stdin, &memory)?;
            interrupt_guard.run_yubikey_operation(|| {
                storage::put(&mut device, options.name, secret.as_slice(), options.force)
            })
        }
        YubikeyCommand::Get(options) => {
            let interrupt_guard = InterruptGuard::install()?;
            let mut device = open_device(options.serial)?;
            verify_pin_from_input(&mut device)?;
            let output_bytes = interrupt_guard
                .run_yubikey_operation(|| storage::get(&mut device, options.name))?;
            write_secret_to_stdout(output_bytes.as_slice())?;
            Ok(())
        }
        YubikeyCommand::EnrollPrimary(options) => {
            require_stdin_secret_source(options.stdin_json, StdinSecretMode::BootstrapJson)?;
            let mut device = open_device(options.serial)?;
            storage::check_setup_preconditions(&mut device)?;
            let memory = SecretMemoryGuard::prepare()?;
            let secrets = read_protected_bootstrap_secrets(options.stdin_json, &memory)?;
            verify_pin_from_input(&mut device)?;
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
/// `--spare-serial` 指定時は secret 復号前に spare の準備状態を確定し、primary と
/// 同一 serial の取り違えを先に拒否する。`--stdin-json` は primary が使えない
/// migration / recovery 用で、この場合だけ stdin の secret 一式を正本として扱う。
fn run_enroll_spare(options: EnrollSpareOptions) -> Result<()> {
    let mut boundary = RealSecretsBoundary;
    run_enroll_spare_with(options, &mut boundary)
}

/// `put` / token 更新の secret は読み取り直後に保護済み値へ移す。
fn read_protected_secret_for_put(
    name: SecretName,
    stdin: bool,
    memory: &SecretMemoryGuard,
) -> Result<ProtectedSecret> {
    let secret = if stdin {
        read_one_stdin_secret(MAX_SINGLE_STDIN_SECRET_LEN)?
    } else {
        read_hidden_secret(&format!("{}: ", name))?
    };
    protect_secret_input(secret, memory)
}

/// 登録用 bootstrap secret は、3 field すべてを保護済み値として組み立てる。
fn read_protected_bootstrap_secrets(
    stdin_json: bool,
    memory: &SecretMemoryGuard,
) -> Result<ProtectedBootstrapSecrets> {
    if stdin_json {
        let input = ProtectedInputBuffer::read_from(
            std::io::stdin(),
            MAX_BOOTSTRAP_JSON_LEN,
            "bootstrap secret JSON input is too large",
            Some(memory),
        )?;
        let secrets = parse_bootstrap_secrets_json(input.as_slice())
            .context("failed to parse bootstrap secret JSON")?;
        return ProtectedBootstrapSecrets::protect(secrets, memory);
    }

    let bw_email = protect_secret_input(
        read_visible_secret_line("bw-email: ", MAX_SINGLE_STDIN_SECRET_LEN)?,
        memory,
    )?;
    let bw_password = read_protected_secret_for_put(SecretName::BwPassword, false, memory)?;
    let bws_access_token =
        read_protected_secret_for_put(SecretName::BwsAccessToken, false, memory)?;

    Ok(ProtectedBootstrapSecrets::new(
        bw_email,
        bw_password,
        bws_access_token,
    ))
}

/// device/input 境界を差し替え、secret 読み込み前の spare 準備と同一 serial 拒否を固定する。
fn run_enroll_spare_with<B: SecretsBoundary>(
    options: EnrollSpareOptions,
    boundary: &mut B,
) -> Result<()> {
    let interrupt_guard = InterruptGuard::install()?;
    let memory = SecretMemoryGuard::prepare()?;
    let prepared_spare = if options.spare_serial.is_some() {
        let mut spare = boundary.open_spare_device(
            options.spare_serial,
            options.primary_serial,
            &interrupt_guard,
        )?;
        interrupt_guard.run_yubikey_operation(|| storage::check_setup_preconditions(&mut spare))?;
        Some(spare)
    } else {
        None
    };
    let (bootstrap, primary_serial, spare) = if options.stdin_json {
        require_spare_serial_for_stdin_json(options.spare_serial, boundary)?;
        interrupt_guard.check_interrupted()?;
        (
            boundary.read_bootstrap_secrets(true, &memory)?,
            options.primary_serial,
            prepared_spare,
        )
    } else {
        let mut primary = boundary.open_device(options.primary_serial)?;
        let primary_serial = primary.serial();
        if prepared_spare
            .as_ref()
            .is_some_and(|spare_device| spare_device.serial() == primary_serial)
        {
            bail!("primary and spare YubiKey serial must be different");
        }
        verify_pin_for_secret_reads(boundary, &mut primary)?;
        let secrets =
            read_protected_bootstrap_from_device(&mut primary, &interrupt_guard, &memory)?;
        (secrets, Some(primary_serial), prepared_spare)
    };

    let mut spare = match spare {
        Some(spare) => spare,
        None => {
            boundary.open_spare_device(options.spare_serial, primary_serial, &interrupt_guard)?
        }
    };

    verify_pin_for_secret_reads(boundary, &mut spare)?;
    let summary = interrupt_guard
        .run_yubikey_operation(|| storage::enroll(&mut spare, YubikeyRole::Spare, &bootstrap))?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

/// primary 復号結果は bootstrap secret 集合として保護済みにしてから登録処理へ渡す。
fn read_protected_bootstrap_from_device<D: SecretDevice>(
    primary: &mut D,
    interrupt_guard: &InterruptGuard,
    memory: &SecretMemoryGuard,
) -> Result<ProtectedBootstrapSecrets> {
    ProtectedBootstrapSecrets::protect(
        BootstrapSecrets {
            bw_email: interrupt_guard
                .run_yubikey_operation(|| storage::get(primary, SecretName::BwEmail))?,
            bw_password: interrupt_guard
                .run_yubikey_operation(|| storage::get(primary, SecretName::BwPassword))?,
            bws_access_token: interrupt_guard
                .run_yubikey_operation(|| storage::get(primary, SecretName::BwsAccessToken))?,
        },
        memory,
    )
}

/// BWS token を 1 本または対話で選んだ複数本の YubiKey に保存する。
///
/// 非対話実行では `--serial` で 1 本だけを更新する。対話実行で serial 指定がない場合は
/// token を一度だけ受け取り、利用者が終了を選ぶまで YubiKey 選択と更新を繰り返す。
fn run_rotate_bws_token(options: RotateBwsTokenOptions) -> Result<()> {
    let mut boundary = RealSecretsBoundary;
    run_rotate_bws_token_with(options, &mut boundary)
}

/// token 入力を 1 回に限定し、複数 YubiKey 更新時も同じ token buffer を再利用する。
fn run_rotate_bws_token_with<B: SecretsBoundary>(
    options: RotateBwsTokenOptions,
    boundary: &mut B,
) -> Result<()> {
    let interrupt_guard = InterruptGuard::install()?;

    if let Some(serial) = options.serial {
        require_stdin_secret_source_for_boundary(options.stdin, StdinSecretMode::Single, boundary)?;
        let memory = SecretMemoryGuard::prepare()?;
        let mut device = boundary.open_device(Some(serial))?;
        verify_pin_for_secret_reads(boundary, &mut device)?;
        interrupt_guard
            .run_yubikey_operation(|| storage::check_rotate_preconditions(&mut device))?;
        let token =
            boundary.read_secret_for_put(SecretName::BwsAccessToken, options.stdin, &memory)?;
        let summary = interrupt_guard
            .run_yubikey_operation(|| storage::rotate_bws_token(&mut device, token.as_slice()))?;
        println!("{}", serde_json::to_string_pretty(&summary)?);
        return Ok(());
    }

    let memory = SecretMemoryGuard::prepare()?;
    let mut device = boundary.open_device(None)?;
    verify_pin_for_secret_reads(boundary, &mut device)?;
    interrupt_guard.run_yubikey_operation(|| storage::check_rotate_preconditions(&mut device))?;
    let token = boundary.read_secret_for_put(SecretName::BwsAccessToken, options.stdin, &memory)?;
    let mut updated_serials = BTreeSet::from([device.serial()]);
    let mut summaries = vec![
        interrupt_guard
            .run_yubikey_operation(|| storage::rotate_bws_token(&mut device, token.as_slice()))?,
    ];

    while interrupt_guard
        .run_yubikey_operation(|| boundary.prompt_yes_no("Update another YubiKey? [y/N] "))?
    {
        let mut device = boundary.open_device(None)?;
        if !updated_serials.insert(device.serial()) {
            bail!("selected YubiKey was already updated");
        }
        verify_pin_for_secret_reads(boundary, &mut device)?;
        interrupt_guard
            .run_yubikey_operation(|| storage::check_rotate_preconditions(&mut device))?;
        summaries.push(
            interrupt_guard.run_yubikey_operation(|| {
                storage::rotate_bws_token(&mut device, token.as_slice())
            })?,
        );
    }

    println!("{}", serde_json::to_string_pretty(&summaries)?);
    Ok(())
}

/// local storage の復号確認を行い、外部確認要求は利用不可として拒否する。
///
/// 引数なしの実行だけが YubiKey 上の bootstrap secret 復号を検証する。
fn run_verify_yubikey(options: VerifyYubikeyOptions) -> Result<()> {
    let mut boundary = RealSecretsBoundary;
    run_verify_yubikey_with(options, &mut boundary)
}

/// 外部 service check は未実装として device access 前に拒否し、local storage 検証だけを通す。
fn run_verify_yubikey_with<B: SecretsBoundary>(
    options: VerifyYubikeyOptions,
    boundary: &mut B,
) -> Result<()> {
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

    let interrupt_guard = InterruptGuard::install()?;
    let mut device = boundary.open_device(options.serial)?;
    verify_pin_for_secret_reads(boundary, &mut device)?;
    let summary =
        interrupt_guard.run_yubikey_operation(|| storage::verify_local_storage(&mut device))?;

    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

/// CLI 統合フローが依存する device/input 境界。
trait SecretsBoundary {
    type Device: SecretDevice;

    fn stdin_is_terminal(&self) -> bool;
    fn open_device(&mut self, serial: Option<u32>) -> Result<Self::Device>;
    fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
        interrupt: &InterruptGuard,
    ) -> Result<Self::Device>;
    fn read_bootstrap_secrets(
        &mut self,
        stdin_json: bool,
        memory: &SecretMemoryGuard,
    ) -> Result<ProtectedBootstrapSecrets>;
    fn read_secret_for_put(
        &mut self,
        name: SecretName,
        stdin: bool,
        memory: &SecretMemoryGuard,
    ) -> Result<ProtectedSecret>;
    fn read_yubikey_pin(&mut self) -> Result<Zeroizing<Vec<u8>>>;
    fn prompt_yes_no(&mut self, prompt: &str) -> Result<bool>;
}

/// CLI は kebab-case 名だけを受け付け、wire format の secret id 変換とは分離する。
fn parse_secret_name(value: &str) -> std::result::Result<SecretName, String> {
    value
        .parse()
        .map_err(|_| format!("unsupported YubiKey secret name: {value}"))
}

/// 非対話実行で prompt の代わりに要求する secret 入力形式。
#[derive(Clone, Copy)]
enum StdinSecretMode {
    Single,
    BootstrapJson,
}

/// 非対話実行では secret prompt を使えないため、stdin 入力 option を先に確定する。
fn require_stdin_secret_source(enabled: bool, mode: StdinSecretMode) -> Result<()> {
    if !enabled && !input_stdin_is_terminal() {
        bail!(stdin_secret_source_error(mode));
    }

    Ok(())
}

/// fake 境界を使う command でも、secret 入力前の非対話契約を同じ判定にそろえる。
fn require_stdin_secret_source_for_boundary<B: SecretsBoundary>(
    enabled: bool,
    mode: StdinSecretMode,
    boundary: &B,
) -> Result<()> {
    if !enabled && !boundary.stdin_is_terminal() {
        bail!(stdin_secret_source_error(mode));
    }

    Ok(())
}

/// `--stdin-json` で primary を読まない経路では、spare prompt も非対話前に禁止する。
fn require_spare_serial_for_stdin_json<B: SecretsBoundary>(
    spare_serial: Option<u32>,
    boundary: &B,
) -> Result<()> {
    if spare_serial.is_none() && !boundary.stdin_is_terminal() {
        bail!(SPARE_SERIAL_NONINTERACTIVE_ERROR);
    }

    Ok(())
}

/// 復号系 operation の前に PIN 入力を orchestration 層で完了させる。
fn verify_pin_from_input<D: SecretDevice>(device: &mut D) -> Result<()> {
    let pin = read_yubikey_pin()?;
    device.verify_pin(&pin)
}

/// fake 境界を使う高水準 command でも、PIN 入力を device adapter の外側に固定する。
fn verify_pin_for_secret_reads<B: SecretsBoundary>(
    boundary: &mut B,
    device: &mut B::Device,
) -> Result<()> {
    let pin = boundary.read_yubikey_pin()?;
    device.verify_pin(&pin)
}

/// command ごとの stdin option 名を error message の単一 source にする。
fn stdin_secret_source_error(mode: StdinSecretMode) -> &'static str {
    match mode {
        StdinSecretMode::Single => "pass --stdin in non-interactive use",
        StdinSecretMode::BootstrapJson => "pass --stdin-json in non-interactive use",
    }
}

struct RealSecretsBoundary;

impl SecretsBoundary for RealSecretsBoundary {
    type Device = device::YubikeySecretDevice;

    fn stdin_is_terminal(&self) -> bool {
        input_stdin_is_terminal()
    }

    fn open_device(&mut self, serial: Option<u32>) -> Result<Self::Device> {
        open_device(serial)
    }

    fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
        interrupt: &InterruptGuard,
    ) -> Result<Self::Device> {
        open_spare_device(spare_serial, primary_serial, interrupt)
    }

    fn read_bootstrap_secrets(
        &mut self,
        stdin_json: bool,
        memory: &SecretMemoryGuard,
    ) -> Result<ProtectedBootstrapSecrets> {
        read_protected_bootstrap_secrets(stdin_json, memory)
    }

    fn read_secret_for_put(
        &mut self,
        name: SecretName,
        stdin: bool,
        memory: &SecretMemoryGuard,
    ) -> Result<ProtectedSecret> {
        read_protected_secret_for_put(name, stdin, memory)
    }

    fn read_yubikey_pin(&mut self) -> Result<Zeroizing<Vec<u8>>> {
        read_yubikey_pin()
    }

    fn prompt_yes_no(&mut self, prompt: &str) -> Result<bool> {
        prompt_yes_no(prompt)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};

    use zeroize::Zeroizing;

    use super::*;
    struct FakeDevice {
        serial: u32,
        key_exists: bool,
        objects: BTreeMap<storage::PivObjectId, Zeroizing<Vec<u8>>>,
    }

    impl FakeDevice {
        fn new(serial: u32) -> Self {
            Self {
                serial,
                key_exists: false,
                objects: BTreeMap::new(),
            }
        }

        fn provisioned(serial: u32) -> Result<Self> {
            let mut device = Self::new(serial);
            storage::setup(&mut device)?;
            storage::put(&mut device, SecretName::BwEmail, b"u@example.com", false)?;
            storage::put(&mut device, SecretName::BwPassword, b"pw", false)?;
            storage::put(&mut device, SecretName::BwsAccessToken, b"token", false)?;
            Ok(device)
        }
    }

    impl SecretDevice for FakeDevice {
        fn serial(&self) -> u32 {
            self.serial
        }

        fn key_exists(&mut self) -> Result<bool> {
            Ok(self.key_exists)
        }

        fn check_key_generation_preconditions(&mut self) -> Result<()> {
            Ok(())
        }

        fn check_management_auth_preconditions(&mut self) -> Result<()> {
            Ok(())
        }

        fn generate_key(&mut self) -> Result<()> {
            self.key_exists = true;
            Ok(())
        }

        fn read_object(
            &mut self,
            object_id: storage::PivObjectId,
        ) -> Result<Option<Zeroizing<Vec<u8>>>> {
            Ok(self.objects.get(&object_id).cloned())
        }

        fn write_object(&mut self, object_id: storage::PivObjectId, value: &[u8]) -> Result<()> {
            self.objects
                .insert(object_id, Zeroizing::new(value.to_vec()));
            Ok(())
        }

        fn wrap_key(&mut self, key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
            Ok(Zeroizing::new(key.iter().map(|byte| byte ^ 0xa5).collect()))
        }

        fn verify_pin(&mut self, _pin: &[u8]) -> Result<()> {
            Ok(())
        }

        fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
            self.wrap_key(wrapped_key)
        }
    }

    struct FakeBoundary {
        stdin_is_terminal: bool,
        devices: VecDeque<FakeDevice>,
        prompts: VecDeque<bool>,
        open_device_calls: usize,
        read_bootstrap_calls: usize,
    }

    impl FakeBoundary {
        fn new(devices: Vec<FakeDevice>) -> Self {
            Self {
                stdin_is_terminal: true,
                devices: devices.into(),
                prompts: VecDeque::new(),
                open_device_calls: 0,
                read_bootstrap_calls: 0,
            }
        }
    }

    impl SecretsBoundary for FakeBoundary {
        type Device = FakeDevice;

        fn stdin_is_terminal(&self) -> bool {
            self.stdin_is_terminal
        }

        fn open_device(&mut self, _serial: Option<u32>) -> Result<Self::Device> {
            self.open_device_calls += 1;
            self.devices
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("no fake device queued"))
        }

        fn open_spare_device(
            &mut self,
            _spare_serial: Option<u32>,
            _primary_serial: Option<u32>,
            _interrupt: &InterruptGuard,
        ) -> Result<Self::Device> {
            self.open_device(None)
        }

        fn read_bootstrap_secrets(
            &mut self,
            _stdin_json: bool,
            memory: &SecretMemoryGuard,
        ) -> Result<ProtectedBootstrapSecrets> {
            self.read_bootstrap_calls += 1;
            ProtectedBootstrapSecrets::protect(
                BootstrapSecrets {
                    bw_email: storage::SecretBytes::new(b"u@example.com".to_vec()),
                    bw_password: storage::SecretBytes::new(b"pw".to_vec()),
                    bws_access_token: storage::SecretBytes::new(b"token".to_vec()),
                },
                memory,
            )
        }

        fn read_secret_for_put(
            &mut self,
            _name: SecretName,
            _stdin: bool,
            memory: &SecretMemoryGuard,
        ) -> Result<ProtectedSecret> {
            protect_secret_input(Zeroizing::new(b"token".to_vec()).into(), memory)
        }

        fn read_yubikey_pin(&mut self) -> Result<Zeroizing<Vec<u8>>> {
            Ok(Zeroizing::new(b"123456".to_vec()))
        }

        fn prompt_yes_no(&mut self, _prompt: &str) -> Result<bool> {
            Ok(self.prompts.pop_front().unwrap_or(false))
        }
    }

    #[test]
    fn enroll_spare_rejects_non_interactive_without_spare_serial_for_stdin_json() {
        let options = EnrollSpareOptions {
            primary_serial: None,
            spare_serial: None,
            stdin_json: true,
        };
        let mut boundary = FakeBoundary::new(vec![]);
        boundary.stdin_is_terminal = false;

        let result = run_enroll_spare_with(options, &mut boundary);

        assert!(result.is_err());
        assert_eq!(
            result.err().map(|err| err.to_string()),
            Some(SPARE_SERIAL_NONINTERACTIVE_ERROR.to_owned())
        );
    }

    #[test]
    fn enroll_spare_rejects_same_primary_and_spare_serial() -> Result<()> {
        let options = EnrollSpareOptions {
            primary_serial: Some(1001),
            spare_serial: Some(1001),
            stdin_json: false,
        };
        let mut boundary =
            FakeBoundary::new(vec![FakeDevice::new(1001), FakeDevice::provisioned(1001)?]);

        let result = run_enroll_spare_with(options, &mut boundary);

        assert!(result.is_err());
        assert_eq!(
            result.err().map(|err| err.to_string()),
            Some("primary and spare YubiKey serial must be different".to_owned())
        );
        Ok(())
    }

    #[test]
    fn rotate_bws_token_rejects_duplicate_serial_in_multi_update() -> Result<()> {
        let options = RotateBwsTokenOptions {
            serial: None,
            stdin: true,
        };
        let mut boundary =
            FakeBoundary::new(vec![FakeDevice::provisioned(2001)?, FakeDevice::new(2001)]);
        boundary.prompts.push_back(true);

        let result = run_rotate_bws_token_with(options, &mut boundary);

        assert!(result.is_err());
        assert_eq!(
            result.err().map(|err| err.to_string()),
            Some("selected YubiKey was already updated".to_owned())
        );
        Ok(())
    }

    #[test]
    fn rotate_bws_token_rejects_non_interactive_serial_without_stdin() -> Result<()> {
        let options = RotateBwsTokenOptions {
            serial: Some(2001),
            stdin: false,
        };
        let mut boundary = FakeBoundary::new(vec![FakeDevice::provisioned(2001)?]);
        boundary.stdin_is_terminal = false;

        let result = run_rotate_bws_token_with(options, &mut boundary);

        assert!(result.is_err());
        assert_eq!(
            result.err().map(|err| err.to_string()),
            Some(stdin_secret_source_error(StdinSecretMode::Single).to_owned())
        );
        assert_eq!(boundary.open_device_calls, 0);
        Ok(())
    }

    #[test]
    fn verify_yubikey_rejects_unsupported_checks_without_device_access() {
        let options = VerifyYubikeyOptions {
            serial: Some(3001),
            check: vec![VerifyCheck::Bws],
            all: false,
        };
        let mut boundary = FakeBoundary::new(vec![]);

        let result = run_verify_yubikey_with(options, &mut boundary);

        assert!(result.is_err());
        assert_eq!(boundary.open_device_calls, 0);
    }

    #[test]
    fn verify_yubikey_rejects_all_without_device_access() {
        let options = VerifyYubikeyOptions {
            serial: Some(3001),
            check: vec![],
            all: true,
        };
        let mut boundary = FakeBoundary::new(vec![]);

        let result = run_verify_yubikey_with(options, &mut boundary);

        assert!(result.is_err());
        assert_eq!(boundary.open_device_calls, 0);
    }

    #[test]
    fn verify_yubikey_rejects_all_and_check_without_device_access() {
        let options = VerifyYubikeyOptions {
            serial: Some(3001),
            check: vec![VerifyCheck::BwLogin],
            all: true,
        };
        let mut boundary = FakeBoundary::new(vec![]);

        let result = run_verify_yubikey_with(options, &mut boundary);

        assert!(result.is_err());
        assert_eq!(boundary.open_device_calls, 0);
    }

    #[test]
    fn enroll_spare_stdin_json_with_spare_serial_reads_bootstrap_once() -> Result<()> {
        let options = EnrollSpareOptions {
            primary_serial: Some(1001),
            spare_serial: Some(1002),
            stdin_json: true,
        };
        let mut boundary = FakeBoundary::new(vec![FakeDevice::new(1002)]);

        run_enroll_spare_with(options, &mut boundary)?;

        assert_eq!(boundary.read_bootstrap_calls, 1);
        Ok(())
    }
}
