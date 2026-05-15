//! `dotfiles secrets` の application 層。
//!
//! storage model は保存形式と secret 値そのものを表す。この層は command ごとの use case
//! flow、device/input 境界、use-case 実行中だけ必要な memory lock 付き所有状態を扱う。

use std::collections::BTreeSet;

use anyhow::{Context, bail};
use zeroize::Zeroizing;

use super::{
    EnrollSpareOptions, SecretsCommand, SecretsOptions, VerifyCheck, VerifyYubikeyOptions,
    YubikeyCommand, YubikeyOptions,
    device::{self, open_device, open_spare_device},
    input::{
        parse_bootstrap_secrets_json, read_hidden_secret, read_one_stdin_secret,
        read_visible_secret_line, read_yubikey_pin, reject_secret_stdout_terminal,
        write_secret_to_stdout,
    },
    storage::{self, BootstrapSecretSource, BootstrapSecrets, SecretDevice, SecretName},
    util::{
        protection::{InterruptGuard, Protected, ProtectedInputBuffer, SecretMemoryGuard},
        terminal::{
            SPARE_SERIAL_NONINTERACTIVE_ERROR, prompt_yes_no,
            stdin_is_terminal as input_stdin_is_terminal,
        },
    },
};
use crate::Result;

const MAX_BOOTSTRAP_JSON_LEN: usize = 64 * 1024;
pub(super) const MAX_SINGLE_STDIN_SECRET_LEN: usize = 16 * 1024;

/// 単一 secret を memory lock guard と同じ生存期間で保持する use-case 状態。
pub(crate) type ProtectedSecret = Protected<storage::SecretBytes>;

/// bootstrap 登録が保存前に要求する 3 種類の保護済み secret。
pub(crate) struct ProtectedBootstrapSecrets {
    bw_email: ProtectedSecret,
    bw_password: ProtectedSecret,
    bws_access_token: ProtectedSecret,
}

impl ProtectedBootstrapSecrets {
    /// prompt 経路で field ごとに保護済みになった値だけを bootstrap 入力として受ける。
    pub(crate) fn new(
        bw_email: ProtectedSecret,
        bw_password: ProtectedSecret,
        bws_access_token: ProtectedSecret,
    ) -> Self {
        Self {
            bw_email,
            bw_password,
            bws_access_token,
        }
    }

    /// JSON や device 復号で得た bootstrap secret は field ごとに lock してから登録へ渡す。
    pub(crate) fn protect(
        secrets: BootstrapSecrets,
        memory: &SecretMemoryGuard,
    ) -> Result<ProtectedBootstrapSecrets> {
        Ok(ProtectedBootstrapSecrets {
            bw_email: protect_secret(secrets.bw_email, memory)?,
            bw_password: protect_secret(secrets.bw_password, memory)?,
            bws_access_token: protect_secret(secrets.bws_access_token, memory)?,
        })
    }
}

impl BootstrapSecretSource for ProtectedBootstrapSecrets {
    /// storage 登録中だけ、要求された bootstrap secret の平文 bytes を借用する。
    fn get(&self, name: SecretName) -> &[u8] {
        match name {
            SecretName::BwEmail => self.bw_email.as_slice(),
            SecretName::BwPassword => self.bw_password.as_slice(),
            SecretName::BwsAccessToken => self.bws_access_token.as_slice(),
        }
    }
}

/// 単一 secret は memory lock 付き状態にしてから storage 操作へ渡す。
pub(crate) fn protect_secret(
    secret: storage::SecretBytes,
    memory: &SecretMemoryGuard,
) -> Result<ProtectedSecret> {
    memory.protect_value(secret, storage::SecretBytes::as_slice)
}

/// command 入力境界で確定した単一 secret を memory lock 付き状態へ移す。
pub(crate) fn protect_secret_input(
    input: super::input::SecretInputBuffer,
    memory: &SecretMemoryGuard,
) -> Result<ProtectedSecret> {
    protect_secret(input.into(), memory)
}

/// CLI で parse 済みの `dotfiles secrets` command を application flow へ渡す。
pub(super) fn run(options: SecretsOptions) -> Result<()> {
    let mut boundary = RealSecretsBoundary;
    run_with_boundary(options, &mut boundary)
}

/// integration test は YubiKey 境界だけを差し替え、CLI 入力境界は実プロセスで通す。
pub(super) fn run_with_boundary<B: SecretsBoundary>(
    options: SecretsOptions,
    boundary: &mut B,
) -> Result<()> {
    match options.command {
        SecretsCommand::Yubikey(options) => run_yubikey_with(options, boundary),
        SecretsCommand::VerifyYubikey(options) => run_verify_yubikey_with(options, boundary),
    }
}

/// `dotfiles secrets yubikey` 配下の command を実行する。
///
/// 低水準 command は単一 secret または storage setup だけを扱い、高水準 command は
/// primary / spare 登録と local verify までを一連の操作として扱う。
fn run_yubikey_with<B: SecretsBoundary>(options: YubikeyOptions, boundary: &mut B) -> Result<()> {
    match options.command {
        YubikeyCommand::Setup(options) => {
            let mut device = boundary.open_device(options.serial)?;
            storage::setup(&mut device)
        }
        YubikeyCommand::Put(options) => {
            require_stdin_secret_source_for_boundary(
                options.stdin,
                StdinSecretMode::Single,
                boundary,
            )?;
            let interrupt_guard = InterruptGuard::install()?;
            let memory = SecretMemoryGuard::prepare()?;
            let mut device = boundary.open_device(options.serial)?;
            interrupt_guard.run_yubikey_operation(|| {
                storage::check_put_preconditions(&mut device, options.name, options.force)
            })?;
            let secret = boundary.read_secret_for_put(options.name, options.stdin, &memory)?;
            interrupt_guard.run_yubikey_operation(|| {
                storage::put(&mut device, options.name, secret.as_slice(), options.force)
            })
        }
        YubikeyCommand::Get(options) => {
            require_secret_stdout_for_boundary(boundary)?;
            let interrupt_guard = InterruptGuard::install()?;
            let mut device = boundary.open_device(options.serial)?;
            verify_pin_for_secret_reads(boundary, &mut device)?;
            let output_bytes = interrupt_guard
                .run_yubikey_operation(|| storage::get(&mut device, options.name))?;
            write_secret_to_stdout(output_bytes.as_slice())?;
            Ok(())
        }
        YubikeyCommand::EnrollPrimary(options) => {
            require_stdin_secret_source_for_boundary(
                options.stdin_json,
                StdinSecretMode::BootstrapJson,
                boundary,
            )?;
            let interrupt_guard = InterruptGuard::install()?;
            let mut device = boundary.open_device(options.serial)?;
            interrupt_guard
                .run_yubikey_operation(|| storage::check_setup_preconditions(&mut device))?;
            let memory = SecretMemoryGuard::prepare()?;
            let summary = {
                let secrets = boundary.read_bootstrap_secrets(options.stdin_json, &memory)?;
                verify_pin_for_secret_reads(boundary, &mut device)?;
                interrupt_guard.run_yubikey_operation(|| {
                    storage::enroll(&mut device, storage::YubikeyRole::Primary, &secrets)
                })?
            };
            println!("{}", serde_json::to_string_pretty(&summary)?);
            Ok(())
        }
        YubikeyCommand::EnrollSpare(options) => run_enroll_spare_with(options, boundary),
        YubikeyCommand::RotateBwsToken(options) => run_rotate_bws_token_with(options, boundary),
    }
}

/// `put` / token 更新の secret は読み取り直後に保護済み値へ移す。
fn read_protected_secret_for_put(
    name: SecretName,
    stdin: bool,
    memory: &SecretMemoryGuard,
) -> Result<ProtectedSecret> {
    let secret = if stdin {
        read_one_stdin_secret(MAX_SINGLE_STDIN_SECRET_LEN, Some(memory))?
    } else {
        read_hidden_secret(&format!("{}: ", name))?
    };
    protect_secret_input(secret, memory)
}

/// 登録用 bootstrap secret は、3 field すべてを保護済み値として組み立てる。
pub(super) fn read_protected_bootstrap_secrets(
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
    let summary = interrupt_guard.run_yubikey_operation(|| {
        storage::enroll(&mut spare, storage::YubikeyRole::Spare, &bootstrap)
    })?;
    drop(bootstrap);
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

/// primary 復号結果は secret ごとに保護済みに移してから次の復号へ進む。
fn read_protected_bootstrap_from_device<D: storage::SecretDevice>(
    primary: &mut D,
    interrupt_guard: &InterruptGuard,
    memory: &SecretMemoryGuard,
) -> Result<ProtectedBootstrapSecrets> {
    let bw_email = protect_secret(
        interrupt_guard.run_yubikey_operation(|| storage::get(primary, SecretName::BwEmail))?,
        memory,
    )?;
    let bw_password = protect_secret(
        interrupt_guard.run_yubikey_operation(|| storage::get(primary, SecretName::BwPassword))?,
        memory,
    )?;
    let bws_access_token = protect_secret(
        interrupt_guard
            .run_yubikey_operation(|| storage::get(primary, SecretName::BwsAccessToken))?,
        memory,
    )?;
    Ok(ProtectedBootstrapSecrets::new(
        bw_email,
        bw_password,
        bws_access_token,
    ))
}

/// token 入力を 1 回に限定し、複数 YubiKey 更新時も同じ token buffer を再利用する。
fn run_rotate_bws_token_with<B: SecretsBoundary>(
    options: super::RotateBwsTokenOptions,
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
        drop(token);
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

    drop(token);
    println!("{}", serde_json::to_string_pretty(&summaries)?);
    Ok(())
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
pub(super) trait SecretsBoundary {
    type Device: storage::SecretDevice;

    fn stdin_is_terminal(&self) -> bool;
    fn stdout_is_terminal(&self) -> bool;
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

/// 非対話実行で prompt の代わりに要求する secret 入力形式。
#[derive(Clone, Copy)]
enum StdinSecretMode {
    Single,
    BootstrapJson,
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

/// `get` は PIN/touch 前に出力先を確定し、TTY へ平文 secret を復号しない。
fn require_secret_stdout_for_boundary<B: SecretsBoundary>(boundary: &B) -> Result<()> {
    if boundary.stdout_is_terminal() {
        reject_secret_stdout_terminal()?;
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

    fn stdout_is_terminal(&self) -> bool {
        super::util::terminal::stdout_is_terminal()
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
