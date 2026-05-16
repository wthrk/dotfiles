//! `dotfiles secrets` の application 層。
//!
//! この層は command ごとの use case と外部境界の順序を所有する。secret を読む前に
//! device と非対話条件を確定し、平文 secret は `SecretSession` に紐づく保護済み値として
//! storage 層へ渡す。

use std::collections::BTreeSet;

use anyhow::{Context, bail};
use zeroize::Zeroizing;

use super::{
    EnrollSpareOptions, SecretsCommand, SecretsOptions, VerifyCheck, VerifyYubikeyOptions,
    YubikeyCommand, YubikeyOptions,
    device::{self, open_device, open_spare_device},
    input::{
        parse_bootstrap_secrets_json, read_hidden_secret, read_visible_secret_line,
        read_yubikey_pin, reject_secret_stdout_terminal, write_secret_to_stdout,
    },
    storage::{self, BootstrapSecretSource, BootstrapSecrets, SecretDevice, SecretName},
    util::{
        protection::{InterruptGuard, Protected, ProtectedInputBuffer, SecretSession},
        terminal::{
            SPARE_SERIAL_NONINTERACTIVE_ERROR, prompt_yes_no,
            stdin_is_terminal as input_stdin_is_terminal,
        },
    },
};
use crate::Result;

const MAX_BOOTSTRAP_JSON_LEN: usize = 64 * 1024;
pub(super) const MAX_SINGLE_STDIN_SECRET_LEN: usize = 16 * 1024;

/// use case 実行中の `SecretSession` より長生きできない単一 secret。
pub(crate) type ProtectedSecret<'session> = Protected<'session, storage::SecretBytes>;

/// bootstrap 登録に必要な 3 field を同一 session の保護境界へ閉じ込める。
pub(crate) struct ProtectedBootstrapSecrets<'session> {
    bw_email: ProtectedSecret<'session>,
    bw_password: ProtectedSecret<'session>,
    bws_access_token: ProtectedSecret<'session>,
}

impl<'session> ProtectedBootstrapSecrets<'session> {
    /// 各 field が同じ `SecretSession` に紐づくことを型で固定する。
    pub(crate) fn new(
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

    /// 未保護の bootstrap model は field 単位で保護済み値へ移してから採用する。
    pub(crate) fn protect(
        secrets: BootstrapSecrets,
        session: &'session SecretSession,
    ) -> Result<ProtectedBootstrapSecrets<'session>> {
        Ok(ProtectedBootstrapSecrets {
            bw_email: protect_secret(secrets.bw_email, session)?,
            bw_password: protect_secret(secrets.bw_password, session)?,
            bws_access_token: protect_secret(secrets.bws_access_token, session)?,
        })
    }
}

impl BootstrapSecretSource for ProtectedBootstrapSecrets<'_> {
    /// 平文 bytes の借用範囲を storage 呼び出し中に限定する。
    fn with_secret<R>(&self, name: SecretName, borrow: impl FnOnce(&[u8]) -> R) -> R {
        match name {
            SecretName::BwEmail => self
                .bw_email
                .with_secret_bytes(storage::SecretBytes::with_secret, borrow),
            SecretName::BwPassword => self
                .bw_password
                .with_secret_bytes(storage::SecretBytes::with_secret, borrow),
            SecretName::BwsAccessToken => self
                .bws_access_token
                .with_secret_bytes(storage::SecretBytes::with_secret, borrow),
        }
    }
}

/// device から復号した単一 secret を現在の session の保護境界へ移す。
pub(crate) fn protect_secret(
    secret: storage::SecretBytes,
    session: &SecretSession,
) -> Result<ProtectedSecret<'_>> {
    session.protect_value(secret, storage::SecretBytes::memory_range)
}

/// prompt 入力の secret buffer を storage model 化し、現在の session へ所属させる。
pub(crate) fn protect_secret_input(
    input: super::input::SecretInputBuffer,
    session: &SecretSession,
) -> Result<ProtectedSecret<'_>> {
    protect_secret(input.into(), session)
}

/// stdin secret は読み込み時の lock guard を引き継ぎ、unlock を値の破棄後に遅延させる。
pub(super) fn read_protected_stdin_secret(
    limit: usize,
    session: &SecretSession,
) -> Result<ProtectedSecret<'_>> {
    let input = ProtectedInputBuffer::read_line_from(std::io::stdin(), limit, Some(session))?;
    let (buffer, lock) =
        input.into_secret_line_and_lock(limit, "stdin secret input is too large")?;
    session.protect_locked_value(buffer.into(), lock)
}

/// 実機境界を使って parse 済み command の use case を開始する。
pub(super) fn run(options: SecretsOptions) -> Result<()> {
    let mut boundary = RealSecretsBoundary;
    run_with_boundary(options, &mut boundary)
}

/// test stub でも実プロセスの TTY / pipe 契約を維持して use case を実行する。
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
/// 低水準 command は単一 secret と storage setup に限定し、高水準 command は
/// primary / spare 登録と local verify までを一連の操作として定義する。
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
            let session = SecretSession::start()?;
            let mut device = boundary.open_device(options.serial)?;
            session.run_yubikey_operation(|| {
                storage::check_put_preconditions(&mut device, options.name, options.force)
            })?;
            let secret = boundary.read_secret_for_put(options.name, options.stdin, &session)?;
            session.run_yubikey_operation(|| {
                secret.with_secret_bytes(storage::SecretBytes::with_secret, |secret| {
                    storage::put(&mut device, options.name, secret, options.force)
                })
            })
        }
        YubikeyCommand::Get(options) => {
            require_secret_stdout_for_boundary(boundary)?;
            let session = SecretSession::start()?;
            let mut device = boundary.open_device(options.serial)?;
            verify_pin_for_secret_reads(boundary, &mut device)?;
            let output_bytes = session
                .run_yubikey_operation(|| storage::get(&mut device, options.name))
                .and_then(|secret| protect_secret(secret, &session))?;
            output_bytes
                .with_secret_bytes(storage::SecretBytes::with_secret, write_secret_to_stdout)?;
            Ok(())
        }
        YubikeyCommand::EnrollPrimary(options) => {
            require_stdin_secret_source_for_boundary(
                options.stdin_json,
                StdinSecretMode::BootstrapJson,
                boundary,
            )?;
            let session = SecretSession::start()?;
            let mut device = boundary.open_device(options.serial)?;
            session.run_yubikey_operation(|| storage::check_setup_preconditions(&mut device))?;
            let summary = {
                let secrets = boundary.read_bootstrap_secrets(options.stdin_json, &session)?;
                session.check_interrupted()?;
                verify_pin_for_secret_reads(boundary, &mut device)?;
                let summary = session.run_yubikey_operation(|| {
                    storage::enroll_without_verify(
                        &mut device,
                        storage::YubikeyRole::Primary,
                        &secrets,
                    )
                })?;
                verify_local_storage_protected(&mut device, &session)?;
                summary
            };
            println!("{}", serde_json::to_string_pretty(&summary)?);
            Ok(())
        }
        YubikeyCommand::EnrollSpare(options) => run_enroll_spare_with(options, boundary),
        YubikeyCommand::RotateBwsToken(options) => run_rotate_bws_token_with(options, boundary),
    }
}

/// 保存対象 secret は入力直後に session 所属の保護済み値へ移す。
fn read_protected_secret_for_put(
    name: SecretName,
    stdin: bool,
    memory: &SecretSession,
) -> Result<ProtectedSecret<'_>> {
    if stdin {
        read_protected_stdin_secret(MAX_SINGLE_STDIN_SECRET_LEN, memory)
    } else {
        protect_secret_input(
            read_hidden_secret(&format!("{}: ", name), MAX_SINGLE_STDIN_SECRET_LEN)?,
            memory,
        )
    }
}

/// bootstrap secret は field ごとの保護境界をそろえてから登録用 model にする。
pub(super) fn read_protected_bootstrap_secrets(
    stdin_json: bool,
    memory: &SecretSession,
) -> Result<ProtectedBootstrapSecrets<'_>> {
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

/// spare 登録では primary 復号前に spare の候補と serial 制約を確定する。
fn run_enroll_spare_with<B: SecretsBoundary>(
    options: EnrollSpareOptions,
    boundary: &mut B,
) -> Result<()> {
    let session = SecretSession::start()?;
    require_primary_serial_for_noninteractive(options.primary_serial, boundary)?;
    require_spare_serial_for_noninteractive(options.spare_serial, boundary)?;
    let prepared_spare = if options.spare_serial.is_some() {
        let mut spare = boundary.open_spare_device(
            options.spare_serial,
            options.primary_serial,
            session.interrupt(),
        )?;
        session.run_yubikey_operation(|| storage::check_setup_preconditions(&mut spare))?;
        Some(spare)
    } else {
        None
    };
    let (bootstrap, primary_serial, spare) = if options.stdin_json {
        session.check_interrupted()?;
        (
            boundary.read_bootstrap_secrets(true, &session)?,
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
        let secrets = read_protected_bootstrap_from_device(&mut primary, &session)?;
        (secrets, Some(primary_serial), prepared_spare)
    };

    session.check_interrupted()?;
    let mut spare = match spare {
        Some(spare) => spare,
        None => {
            boundary.open_spare_device(options.spare_serial, primary_serial, session.interrupt())?
        }
    };

    session.check_interrupted()?;
    verify_pin_for_secret_reads(boundary, &mut spare)?;
    let summary = session.run_yubikey_operation(|| {
        storage::enroll_without_verify(&mut spare, storage::YubikeyRole::Spare, &bootstrap)
    })?;
    verify_local_storage_protected(&mut spare, &session)?;
    drop(bootstrap);
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

/// primary から復号した各 field は、次の device 操作前に保護済み値へ移す。
fn read_protected_bootstrap_from_device<'session, D: storage::SecretDevice>(
    primary: &mut D,
    session: &'session SecretSession,
) -> Result<ProtectedBootstrapSecrets<'session>> {
    let bw_email = protect_secret(
        session.run_yubikey_operation(|| storage::get(primary, SecretName::BwEmail))?,
        session,
    )?;
    let bw_password = protect_secret(
        session.run_yubikey_operation(|| storage::get(primary, SecretName::BwPassword))?,
        session,
    )?;
    let bws_access_token = protect_secret(
        session.run_yubikey_operation(|| storage::get(primary, SecretName::BwsAccessToken))?,
        session,
    )?;
    Ok(ProtectedBootstrapSecrets::new(
        bw_email,
        bw_password,
        bws_access_token,
    ))
}

/// BWS token rotation は 1 回読んだ token を複数 device 更新に再利用する。
fn run_rotate_bws_token_with<B: SecretsBoundary>(
    options: super::RotateBwsTokenOptions,
    boundary: &mut B,
) -> Result<()> {
    let session = SecretSession::start()?;

    if let Some(serial) = options.serial {
        require_stdin_secret_source_for_boundary(options.stdin, StdinSecretMode::Single, boundary)?;
        let mut device = boundary.open_device(Some(serial))?;
        verify_pin_for_secret_reads(boundary, &mut device)?;
        check_rotate_preconditions_protected(&mut device, &session)?;
        let token =
            boundary.read_secret_for_put(SecretName::BwsAccessToken, options.stdin, &session)?;
        session.run_yubikey_operation(|| {
            token.with_secret_bytes(storage::SecretBytes::with_secret, |token| {
                storage::replace_bws_token(&mut device, token)
            })
        })?;
        let summary = verify_local_storage_protected(&mut device, &session)?;
        drop(token);
        println!("{}", serde_json::to_string_pretty(&summary)?);
        return Ok(());
    }

    let mut device = boundary.open_device(None)?;
    verify_pin_for_secret_reads(boundary, &mut device)?;
    check_rotate_preconditions_protected(&mut device, &session)?;
    let token =
        boundary.read_secret_for_put(SecretName::BwsAccessToken, options.stdin, &session)?;
    let mut updated_serials = BTreeSet::from([device.serial()]);
    session.run_yubikey_operation(|| {
        token.with_secret_bytes(storage::SecretBytes::with_secret, |token| {
            storage::replace_bws_token(&mut device, token)
        })
    })?;
    let mut summaries = vec![verify_local_storage_protected(&mut device, &session)?];
    drop(device);

    let remaining_result = (|| -> Result<()> {
        while session
            .run_yubikey_operation(|| boundary.prompt_yes_no("Update another YubiKey? [y/N] "))?
        {
            session.check_interrupted()?;
            let mut device = boundary.open_device(None)?;
            session.check_interrupted()?;
            if !updated_serials.insert(device.serial()) {
                bail!("selected YubiKey was already updated");
            }
            verify_pin_for_secret_reads(boundary, &mut device)?;
            check_rotate_preconditions_protected(&mut device, &session)?;
            session.run_yubikey_operation(|| {
                token.with_secret_bytes(storage::SecretBytes::with_secret, |token| {
                    storage::replace_bws_token(&mut device, token)
                })
            })?;
            summaries.push(verify_local_storage_protected(&mut device, &session)?);
        }
        Ok(())
    })();

    if let Err(err) = remaining_result {
        write_partial_rotate_bws_token_summary(&summaries)?;
        return Err(err);
    }

    drop(token);
    println!("{}", serde_json::to_string_pretty(&summaries)?);
    Ok(())
}

#[derive(serde::Serialize)]
struct PartialRotateBwsTokenSummary<'a> {
    updated: &'a [storage::VerifySummary],
}

/// 部分更新が起きた rotation 失敗では、再実行対象を判別できる JSON を標準出力へ残す。
fn write_partial_rotate_bws_token_summary(summaries: &[storage::VerifySummary]) -> Result<()> {
    if summaries.is_empty() {
        return Ok(());
    }

    let partial = PartialRotateBwsTokenSummary { updated: summaries };
    println!("{}", serde_json::to_string_pretty(&partial)?);
    Ok(())
}

/// 外部 service check は device touch 前に拒否し、未実装項目で secret を復号しない。
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

    let session = SecretSession::start()?;
    let mut device = boundary.open_device(options.serial)?;
    verify_pin_for_secret_reads(boundary, &mut device)?;
    let summary = verify_local_storage_protected(&mut device, &session)?;

    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

/// local verify の復号結果は、空判定前に session の保護境界へ移す。
fn verify_local_storage_protected<D: storage::SecretDevice>(
    device: &mut D,
    session: &SecretSession,
) -> Result<storage::VerifySummary> {
    for name in SecretName::iter() {
        let secret = session
            .run_yubikey_operation(|| storage::get(device, name))
            .and_then(|secret| protect_secret(secret, session))?;
        secret.with_secret_bytes(storage::SecretBytes::with_secret, |secret| {
            if secret.is_empty() {
                bail!("{} stored on this YubiKey is empty", name);
            }
            Ok(())
        })?;
    }

    Ok(storage::VerifySummary {
        serial: device.serial(),
        checks: [
            (storage::CheckName::LocalStorage, storage::CheckStatus::Ok),
            (storage::CheckName::Bws, storage::CheckStatus::Skipped),
            (storage::CheckName::BwLogin, storage::CheckStatus::Skipped),
        ]
        .into_iter()
        .collect(),
    })
}

/// rotation は token 入力前に local verify と management auth を保護境界内で確認する。
fn check_rotate_preconditions_protected<D: storage::SecretDevice>(
    device: &mut D,
    session: &SecretSession,
) -> Result<()> {
    verify_local_storage_protected(device, session)?;
    session.run_yubikey_operation(|| device.check_management_auth_preconditions())
}

/// application 層が実機 I/O と test stub を同じ use case に接続する境界。
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
    fn read_bootstrap_secrets<'session>(
        &mut self,
        stdin_json: bool,
        memory: &'session SecretSession,
    ) -> Result<ProtectedBootstrapSecrets<'session>>;
    fn read_secret_for_put<'session>(
        &mut self,
        name: SecretName,
        stdin: bool,
        memory: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>>;
    fn read_yubikey_pin(&mut self) -> Result<Zeroizing<Vec<u8>>>;
    fn prompt_yes_no(&mut self, prompt: &str) -> Result<bool>;
}

/// 非対話実行時に prompt の代替として許可する secret 入力形式。
#[derive(Clone, Copy)]
enum StdinSecretMode {
    Single,
    BootstrapJson,
}

/// secret 読み込み前に、実機境界と test 境界で同じ非対話契約を適用する。
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

/// 非対話 spare 登録は primary device を prompt なしで特定できる場合に開始する。
fn require_primary_serial_for_noninteractive<B: SecretsBoundary>(
    primary_serial: Option<u32>,
    boundary: &B,
) -> Result<()> {
    if primary_serial.is_none() && !boundary.stdin_is_terminal() {
        bail!("pass --primary-serial in non-interactive use");
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

/// 非対話 spare 登録は差し替え prompt を使わず spare device を特定できる場合に進める。
fn require_spare_serial_for_noninteractive<B: SecretsBoundary>(
    spare_serial: Option<u32>,
    boundary: &B,
) -> Result<()> {
    if spare_serial.is_none() && !boundary.stdin_is_terminal() {
        bail!(SPARE_SERIAL_NONINTERACTIVE_ERROR);
    }

    Ok(())
}

/// PIN 入力は device adapter へ閉じ込めず、application の入力順序で取得する。
fn verify_pin_for_secret_reads<B: SecretsBoundary>(
    boundary: &mut B,
    device: &mut B::Device,
) -> Result<()> {
    let pin = boundary.read_yubikey_pin()?;
    device.verify_pin(&pin)
}

/// stdin 契約違反の error message を command option 名と一致させる。
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

    fn read_bootstrap_secrets<'session>(
        &mut self,
        stdin_json: bool,
        memory: &'session SecretSession,
    ) -> Result<ProtectedBootstrapSecrets<'session>> {
        read_protected_bootstrap_secrets(stdin_json, memory)
    }

    fn read_secret_for_put<'session>(
        &mut self,
        name: SecretName,
        stdin: bool,
        memory: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>> {
        read_protected_secret_for_put(name, stdin, memory)
    }

    fn read_yubikey_pin(&mut self) -> Result<Zeroizing<Vec<u8>>> {
        read_yubikey_pin()
    }

    fn prompt_yes_no(&mut self, prompt: &str) -> Result<bool> {
        prompt_yes_no(prompt)
    }
}
