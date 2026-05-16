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
    device::{self, SPARE_SERIAL_NONINTERACTIVE_ERROR, open_device, open_spare_device},
    input::{
        parse_bootstrap_secrets_json, read_hidden_secret, read_visible_secret_line,
        read_yubikey_pin, reject_secret_stdout_terminal, write_secret_to_stdout,
    },
    storage::{self, BootstrapSecretSource, BootstrapSecrets, SecretDevice, SecretName},
    util::{
        protection::{InterruptGuard, Protected, ProtectedInputBuffer, SecretSession},
        terminal::{prompt_yes_no, stdin_is_terminal as input_stdin_is_terminal},
    },
};
use crate::Result;

const MAX_BOOTSTRAP_JSON_LEN: usize = 64 * 1024;
pub(super) const MAX_SINGLE_STDIN_SECRET_LEN: usize = 16 * 1024;

/// 単一 secret を `SecretSession` の保護境界に所属させる所有値。
///
/// 値の lifetime は use case 実行中の session より長くできない。
pub(crate) type ProtectedSecret<'session> = Protected<'session, storage::SecretBytes>;

/// bootstrap 登録に必要な 3 field を同じ保護 session で所有する。
pub(crate) struct ProtectedBootstrapSecrets<'session> {
    bw_email: ProtectedSecret<'session>,
    bw_password: ProtectedSecret<'session>,
    bws_access_token: ProtectedSecret<'session>,
}

impl<'session> ProtectedBootstrapSecrets<'session> {
    /// 同じ `SecretSession` に所属する 3 field から bootstrap 入力を構築する。
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

    /// 未保護の bootstrap model を field 単位で session 所属の値へ移す。
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
    /// 指定された secret の平文 bytes を closure へ貸し出す。
    ///
    /// 借用範囲は storage 呼び出し中に限定し、所有値を直接取り出させない。
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

/// 復号済み secret を現在の session の保護境界へ移す。
pub(crate) fn protect_secret(
    secret: storage::SecretBytes,
    session: &SecretSession,
) -> Result<ProtectedSecret<'_>> {
    session.protect_value(secret, storage::SecretBytes::memory_range)
}

/// prompt 入力 buffer を storage model へ変換し、現在の session へ所属させる。
pub(crate) fn protect_secret_input(
    input: super::input::SecretInputBuffer,
    session: &SecretSession,
) -> Result<ProtectedSecret<'_>> {
    protect_secret(input.into(), session)
}

/// stdin から 1 secret を読み、現在の session の保護済み値として返す。
///
/// 読み込み時の lock guard を引き継ぎ、unlock は値の破棄後に遅延させる。
pub(super) fn read_protected_stdin_secret(
    limit: usize,
    session: &SecretSession,
) -> Result<ProtectedSecret<'_>> {
    let input = ProtectedInputBuffer::read_line_from(std::io::stdin(), limit, Some(session))?;
    let (buffer, lock) =
        input.into_secret_line_and_lock(limit, "stdin secret input is too large")?;
    session.protect_locked_value(buffer.into(), lock)
}

/// 実機境界を使って parse 済み options の use case を開始する。
pub(super) fn run(options: SecretsOptions) -> Result<()> {
    let mut boundary = RealSecretsBoundary;
    run_with_boundary(options, &mut boundary)
}

/// 指定された外部境界を使って parse 済み options の use case を実行する。
///
/// test stub でも実プロセスの TTY / pipe 契約を同じ境界 trait に通す。
pub(super) fn run_with_boundary<B: SecretsBoundary>(
    options: SecretsOptions,
    boundary: &mut B,
) -> Result<()> {
    match options.command {
        SecretsCommand::Yubikey(options) => run_yubikey_with(options, boundary),
        SecretsCommand::VerifyYubikey(options) => run_verify_yubikey_with(options, boundary),
    }
}

/// `dotfiles secrets yubikey` 配下の command を use case へ dispatch する。
///
/// 単一 secret 操作、storage setup、primary / spare 登録、token rotation をそれぞれ
/// 対応する use case へ分岐する。
fn run_yubikey_with<B: SecretsBoundary>(options: YubikeyOptions, boundary: &mut B) -> Result<()> {
    match options.command {
        YubikeyCommand::Setup(options) => run_setup_with(options, boundary),
        YubikeyCommand::Put(options) => run_put_with(options, boundary),
        YubikeyCommand::Get(options) => run_get_with(options, boundary),
        YubikeyCommand::EnrollPrimary(options) => run_enroll_primary_with(options, boundary),
        YubikeyCommand::EnrollSpare(options) => run_enroll_spare_with(options, boundary),
        YubikeyCommand::RotateBwsToken(options) => run_rotate_bws_token_with(options, boundary),
    }
}

/// `setup` 用の device を開き、storage setup を実行する。
///
/// PIV 領域の衝突検出は storage 層に委ねる。
fn run_setup_with<B: SecretsBoundary>(
    options: super::SerialOptions,
    boundary: &mut B,
) -> Result<()> {
    let mut device = boundary.open_device(options.serial)?;
    storage::setup(&mut device)
}

/// 単一 secret を読み込み、指定された storage object へ保存する。
///
/// 既存 object の上書き可否は secret 入力より前に確定する。
fn run_put_with<B: SecretsBoundary>(options: super::PutOptions, boundary: &mut B) -> Result<()> {
    require_stdin_secret_source_for_boundary(options.stdin, StdinSecretMode::Single, boundary)?;
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

/// 指定された secret を device から復号し、stdout へ書き込む。
///
/// stdout が pipe/redirect でない場合は、PIN verification と touch の前に停止する。
fn run_get_with<B: SecretsBoundary>(options: super::GetOptions, boundary: &mut B) -> Result<()> {
    require_secret_stdout_for_boundary(boundary)?;
    let session = SecretSession::start()?;
    let mut device = boundary.open_device(options.serial)?;
    verify_pin_for_secret_reads(boundary, &mut device)?;
    let output_bytes = session
        .run_yubikey_operation(|| storage::get(&mut device, options.name))
        .and_then(|secret| protect_secret(secret, &session))?;
    output_bytes.with_secret_bytes(storage::SecretBytes::with_secret, write_secret_to_stdout)?;
    Ok(())
}

/// primary 用 bootstrap secrets を読み込み、device へ登録して local verify まで実行する。
///
/// storage 衝突確認が終わるまでは bootstrap secrets を読み始めない。
fn run_enroll_primary_with<B: SecretsBoundary>(
    options: super::EnrollPrimaryOptions,
    boundary: &mut B,
) -> Result<()> {
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
            storage::enroll_without_verify(&mut device, storage::YubikeyRole::Primary, &secrets)
        })?;
        verify_local_storage_protected(&mut device, &session)?;
        summary
    };
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

/// `put` 用の単一 secret を prompt または stdin から読み込む。
///
/// 読み込んだ直後に session 所属の保護済み値へ移す。
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

/// bootstrap 登録用の 3 field を prompt または stdin JSON から読み込む。
///
/// field ごとの保護境界を同じ session にそろえてから登録用 model にする。
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

/// spare 用 bootstrap secrets を取得し、別 device へ登録して local verify まで実行する。
///
/// primary から復号する経路では、復号前に spare 候補と serial 制約を確定する。
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

/// primary device から bootstrap 登録用の 3 field を復号する。
///
/// 各 field は次の device 操作前に session 所属の保護済み値へ移す。
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

/// BWS access token を読み込み、1 本または複数本の device へ反映する。
///
/// 複数更新では 1 回読んだ token を session 内で再利用する。
fn run_rotate_bws_token_with<B: SecretsBoundary>(
    options: super::RotateBwsTokenOptions,
    boundary: &mut B,
) -> Result<()> {
    let session = SecretSession::start()?;

    if let Some(serial) = options.serial {
        require_stdin_secret_source_for_boundary(options.stdin, StdinSecretMode::Single, boundary)?;
        let mut device = prepare_bws_token_rotation_device(boundary, Some(serial), &session)?;
        let token =
            boundary.read_secret_for_put(SecretName::BwsAccessToken, options.stdin, &session)?;
        let summary = rotate_bws_token_on_device(&mut device, &token, &session)?;
        drop(token);
        println!("{}", serde_json::to_string_pretty(&summary)?);
        return Ok(());
    }

    let mut device = prepare_bws_token_rotation_device(boundary, None, &session)?;
    let token =
        boundary.read_secret_for_put(SecretName::BwsAccessToken, options.stdin, &session)?;
    let mut updated_serials = BTreeSet::from([device.serial()]);
    let mut summaries = vec![rotate_bws_token_on_device(&mut device, &token, &session)?];
    drop(device);

    let remaining_result = (|| -> Result<()> {
        while session
            .run_yubikey_operation(|| boundary.prompt_yes_no("Update another YubiKey? [y/N] "))?
        {
            session.check_interrupted()?;
            let mut device = prepare_bws_token_rotation_device(boundary, None, &session)?;
            session.check_interrupted()?;
            if !updated_serials.insert(device.serial()) {
                bail!("selected YubiKey was already updated");
            }
            summaries.push(rotate_bws_token_on_device(&mut device, &token, &session)?);
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

/// BWS token rotation の対象 device を開き、更新前条件を確認する。
///
/// token 入力前に既存 secrets の復号確認と management auth を済ませる。
fn prepare_bws_token_rotation_device<B: SecretsBoundary>(
    boundary: &mut B,
    serial: Option<u32>,
    session: &SecretSession,
) -> Result<B::Device> {
    let mut device = boundary.open_device(serial)?;
    verify_pin_for_secret_reads(boundary, &mut device)?;
    check_rotate_preconditions_protected(&mut device, session)?;
    Ok(device)
}

/// 1 本の device へ BWS access token を書き込み、local verify を実行する。
///
/// token の平文借用範囲は storage 書き込み呼び出し中に限定する。
fn rotate_bws_token_on_device<D: storage::SecretDevice>(
    device: &mut D,
    token: &ProtectedSecret<'_>,
    session: &SecretSession,
) -> Result<storage::VerifySummary> {
    session.run_yubikey_operation(|| {
        token.with_secret_bytes(storage::SecretBytes::with_secret, |token| {
            storage::replace_bws_token(device, token)
        })
    })?;
    verify_local_storage_protected(device, session)
}

#[derive(serde::Serialize)]
struct PartialRotateBwsTokenSummary<'a> {
    updated: &'a [storage::VerifySummary],
}

/// rotation 済み device の summary を部分成功 JSON として stdout へ出力する。
///
/// 途中失敗時に、利用者が再実行対象を判別できる情報を残す。
fn write_partial_rotate_bws_token_summary(summaries: &[storage::VerifySummary]) -> Result<()> {
    if summaries.is_empty() {
        return Ok(());
    }

    let partial = PartialRotateBwsTokenSummary { updated: summaries };
    println!("{}", serde_json::to_string_pretty(&partial)?);
    Ok(())
}

/// YubiKey local storage の verify を実行し、summary JSON を出力する。
///
/// 未実装の外部 service check は device touch 前に拒否する。
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

/// device 上の local storage secrets を復号し、空でないことを確認する。
///
/// 復号結果は空判定前に session の保護境界へ移す。
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

/// rotation の書き込み前条件として local verify と management auth を確認する。
///
/// 確認は token 入力前に現在の保護境界内で実行する。
fn check_rotate_preconditions_protected<D: storage::SecretDevice>(
    device: &mut D,
    session: &SecretSession,
) -> Result<()> {
    verify_local_storage_protected(device, session)?;
    session.run_yubikey_operation(|| device.check_management_auth_preconditions())
}

/// application use case が利用する外部 I/O 境界。
///
/// 実機 adapter と test stub は同じ非対話条件、入力順序、device 操作順序をこの trait で共有する。
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

/// 非対話実行時に prompt の代替として許可する入力形式。
#[derive(Clone, Copy)]
enum StdinSecretMode {
    Single,
    BootstrapJson,
}

/// prompt 入力が必要な use case で、非対話時の代替入力が有効か確認する。
///
/// 実機境界と test 境界で同じ非対話契約を適用する。
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

/// 非対話 spare 登録で primary device を prompt なしに特定できるか確認する。
fn require_primary_serial_for_noninteractive<B: SecretsBoundary>(
    primary_serial: Option<u32>,
    boundary: &B,
) -> Result<()> {
    if primary_serial.is_none() && !boundary.stdin_is_terminal() {
        bail!("pass --primary-serial in non-interactive use");
    }

    Ok(())
}

/// secret 出力先として stdout が安全か確認する。
///
/// TTY の場合は PIN/touch 前に停止する。
fn require_secret_stdout_for_boundary<B: SecretsBoundary>(boundary: &B) -> Result<()> {
    if boundary.stdout_is_terminal() {
        reject_secret_stdout_terminal()?;
    }

    Ok(())
}

/// 非対話 spare 登録で spare device を prompt なしに特定できるか確認する。
fn require_spare_serial_for_noninteractive<B: SecretsBoundary>(
    spare_serial: Option<u32>,
    boundary: &B,
) -> Result<()> {
    if spare_serial.is_none() && !boundary.stdin_is_terminal() {
        bail!(SPARE_SERIAL_NONINTERACTIVE_ERROR);
    }

    Ok(())
}

/// PIN を入力境界から読み取り、device の PIV session を検証する。
///
/// PIN 入力は device adapter に閉じ込めず、application が決める入力順序で取得する。
fn verify_pin_for_secret_reads<B: SecretsBoundary>(
    boundary: &mut B,
    device: &mut B::Device,
) -> Result<()> {
    let pin = boundary.read_yubikey_pin()?;
    device.verify_pin(&pin)
}

/// 非対話入力の不足時に表示する error message を返す。
///
/// message は利用者が指定すべき CLI option 名と一致させる。
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

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};

    use zeroize::Zeroizing;

    use super::*;

    struct FakeBoundary {
        devices: VecDeque<FakeDevice>,
        prompts: VecDeque<bool>,
    }

    impl FakeBoundary {
        fn new(devices: Vec<FakeDevice>) -> Self {
            Self {
                devices: devices.into(),
                prompts: VecDeque::new(),
            }
        }

        fn with_prompts(mut self, prompts: Vec<bool>) -> Self {
            self.prompts = prompts.into();
            self
        }
    }

    impl SecretsBoundary for FakeBoundary {
        type Device = FakeDevice;

        fn stdin_is_terminal(&self) -> bool {
            true
        }

        fn stdout_is_terminal(&self) -> bool {
            false
        }

        fn open_device(&mut self, serial: Option<u32>) -> Result<Self::Device> {
            let mut device = self
                .devices
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("fake device queue is empty"))?;
            if let Some(serial) = serial {
                device.serial = serial;
            }
            Ok(device)
        }

        fn open_spare_device(
            &mut self,
            spare_serial: Option<u32>,
            _primary_serial: Option<u32>,
            _interrupt: &InterruptGuard,
        ) -> Result<Self::Device> {
            self.open_device(spare_serial)
        }

        fn read_bootstrap_secrets<'session>(
            &mut self,
            _stdin_json: bool,
            memory: &'session SecretSession,
        ) -> Result<ProtectedBootstrapSecrets<'session>> {
            ProtectedBootstrapSecrets::protect(bootstrap_secrets(), memory)
        }

        fn read_secret_for_put<'session>(
            &mut self,
            _name: SecretName,
            _stdin: bool,
            memory: &'session SecretSession,
        ) -> Result<ProtectedSecret<'session>> {
            protect_secret(storage::SecretBytes::new(b"rotated-token".to_vec()), memory)
        }

        fn read_yubikey_pin(&mut self) -> Result<Zeroizing<Vec<u8>>> {
            Ok(Zeroizing::new(b"123456".to_vec()))
        }

        fn prompt_yes_no(&mut self, _prompt: &str) -> Result<bool> {
            self.prompts
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("fake prompt queue is empty"))
        }
    }

    struct FakeDevice {
        serial: u32,
        key_exists: bool,
        objects: BTreeMap<storage::PivObjectId, Zeroizing<Vec<u8>>>,
    }

    impl FakeDevice {
        fn fresh(serial: u32) -> Self {
            Self {
                serial,
                key_exists: false,
                objects: BTreeMap::new(),
            }
        }

        fn provisioned(serial: u32) -> Result<Self> {
            let mut device = Self::fresh(serial);
            storage::enroll_without_verify(
                &mut device,
                storage::YubikeyRole::Primary,
                &bootstrap_secrets(),
            )?;
            Ok(device)
        }
    }

    impl storage::SecretDevice for FakeDevice {
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

    #[test]
    fn enroll_spare_rejects_same_primary_and_spare_serial() -> Result<()> {
        let mut boundary =
            FakeBoundary::new(vec![FakeDevice::fresh(10), FakeDevice::provisioned(10)?]);
        let options = EnrollSpareOptions {
            primary_serial: Some(10),
            spare_serial: Some(10),
            stdin_json: false,
        };

        let err = run_enroll_spare_with(options, &mut boundary)
            .err()
            .ok_or_else(|| anyhow::anyhow!("enroll-spare accepted duplicate serials"))?;

        assert_eq!(
            err.to_string(),
            "primary and spare YubiKey serial must be different"
        );
        Ok(())
    }

    #[test]
    fn rotate_bws_token_rejects_already_updated_serial() -> Result<()> {
        let mut boundary = FakeBoundary::new(vec![
            FakeDevice::provisioned(10)?,
            FakeDevice::provisioned(10)?,
        ])
        .with_prompts(vec![true]);
        let options = super::super::RotateBwsTokenOptions {
            serial: None,
            stdin: false,
        };

        let err = run_rotate_bws_token_with(options, &mut boundary)
            .err()
            .ok_or_else(|| anyhow::anyhow!("rotate-bws-token accepted duplicate serials"))?;

        assert_eq!(err.to_string(), "selected YubiKey was already updated");
        Ok(())
    }

    fn bootstrap_secrets() -> storage::BootstrapSecrets {
        storage::BootstrapSecrets {
            bw_email: storage::SecretBytes::new(b"user@example.com".to_vec()),
            bw_password: storage::SecretBytes::new(b"password".to_vec()),
            bws_access_token: storage::SecretBytes::new(b"token".to_vec()),
        }
    }
}
