//! `dotfiles secrets` の application 層。
//!
//! この層は command ごとの use case と外部境界の順序を所有する。secret を読む前に
//! device と非対話条件を確定し、平文 secret は `SecretSession` に紐づく保護済み値として
//! domain の保存操作へ渡す。

pub(crate) mod blob_crypto;
mod storage_service;

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use super::{
    EnrollSpareOptions, SecretsCommand, SecretsOptions, VerifyCheck, VerifyYubikeyOptions,
    YubikeyCommand, YubikeyOptions, adapters,
    domain::SecretName,
    ports::{SecretDevice, SecretsBoundary},
    support::protection::{InterruptGuard, ProtectedSecret, SecretSession},
};
use crate::Result;
use anyhow::bail;

const NONINTERACTIVE_SERIAL_ERROR: &str = "pass --serial in non-interactive use";
const NONINTERACTIVE_PRIMARY_SERIAL_ERROR: &str = "pass --primary-serial in non-interactive use";
const NONINTERACTIVE_SPARE_SERIAL_ERROR: &str = "pass --spare-serial in non-interactive use";
const STDIN_JSON_TTY_ERROR: &str = "--stdin-json requires pipe or redirect input";

/// 登録に必要な 3 field を同じ保護 session で所有する。
pub(crate) struct EnrollmentSecretSet<'session> {
    pub(crate) bw_email: ProtectedSecret<'session>,
    pub(crate) bw_password: ProtectedSecret<'session>,
    pub(crate) bws_access_token: ProtectedSecret<'session>,
}

impl<'session> EnrollmentSecretSet<'session> {
    /// 同じ `SecretSession` に所属する 3 field から登録対象 secret を構築する。
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
}

/// application use case が要求する対話 I/O 境界。
pub(crate) trait InteractionBoundary: SecretsBoundary {
    fn stdin_is_terminal(&self) -> bool;
    fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
        interrupt: &InterruptGuard,
    ) -> Result<Self::Device>;
    fn read_enrollment_secret_set<'session>(
        &mut self,
        stdin_json: bool,
        memory: &'session SecretSession,
    ) -> Result<EnrollmentSecretSet<'session>>;
    fn read_secret_for_put<'session>(
        &mut self,
        name: SecretName,
        stdin: bool,
        memory: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>>;
    fn prompt_yes_no(&mut self, prompt: &str, interrupt: &InterruptGuard) -> Result<bool>;
    fn write_summary_json(&mut self, summary: &impl serde::Serialize) -> Result<()>;
    fn write_secret_to_stdout(&mut self, bytes: &[u8]) -> Result<()>;
    fn ensure_secret_stdout_not_terminal(&self) -> Result<()>;
    fn device_serial(&self, device: &Self::Device) -> u32;
    fn verify_pin_for_secret_reads(
        &mut self,
        device: &mut Self::Device,
        session: &SecretSession,
    ) -> Result<()>;
    fn check_management_auth_preconditions(
        &mut self,
        device: &mut Self::Device,
        session: &SecretSession,
    ) -> Result<()>;
}

/// enroll/verify の summary に出す確認項目の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub(crate) enum CheckStatus {
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "skipped")]
    Skipped,
}

/// summary JSON の `checks` key として使う閉じた確認項目名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CheckName {
    Setup,
    BwEmail,
    BwPassword,
    BwsAccessToken,
    LocalStorage,
    Bws,
    BwLogin,
}

/// YubiKey を primary/spare のどちらとして登録したかを表す role。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum YubikeyRole {
    Primary,
    Spare,
}

/// enroll 系 command の成功 summary。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct EnrollSummary {
    pub serial: u32,
    pub role: YubikeyRole,
    pub checks: BTreeMap<CheckName, CheckStatus>,
}

/// verify 系 command の成功 summary。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct VerifySummary {
    pub serial: u32,
    pub checks: BTreeMap<CheckName, CheckStatus>,
}

/// 指定された device backend を使って parse 済み options の use case を開始する。
///
/// 実行時に選択した device backend と実プロセス I/O 境界を組み合わせ、
/// use case 本体には共通の境界 trait だけを渡す。
pub(super) fn run(options: SecretsOptions, backend: adapters::DeviceBackend) -> Result<()> {
    let mut boundary = adapters::real_boundary::RealSecretsBoundary { backend };
    run_with_boundary(options, &mut boundary)
}

/// 指定された外部境界を使って parse 済み options の use case を実行する。
///
/// 境界実装を差し替えても、TTY / pipe などの入出力契約はこの trait 経由で統一して扱う。
pub(super) fn run_with_boundary<B: SecretsBoundary + InteractionBoundary>(
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
fn run_yubikey_with<B: SecretsBoundary + InteractionBoundary>(
    options: YubikeyOptions,
    boundary: &mut B,
) -> Result<()> {
    match options.command {
        YubikeyCommand::Setup(options) => run_setup_with(options, boundary),
        YubikeyCommand::Put(options) => run_put_with(options, boundary),
        YubikeyCommand::Get(options) => run_get_with(options, boundary),
        YubikeyCommand::EnrollPrimary(options) => run_enroll_primary_with(options, boundary),
        YubikeyCommand::EnrollSpare(options) => run_enroll_spare_with(options, boundary),
        YubikeyCommand::RotateBwsToken(options) => run_rotate_bws_token_with(options, boundary),
    }
}

/// `setup` 用の device を開き、PIV object setup を実行する。
///
/// PIV 領域の衝突検出は domain 層に委ねる。
fn run_setup_with<B: SecretsBoundary + InteractionBoundary>(
    options: super::SerialOptions,
    boundary: &mut B,
) -> Result<()> {
    require_noninteractive_serial(boundary, options.serial, NONINTERACTIVE_SERIAL_ERROR)?;
    let mut device = boundary.open_device(options.serial)?;
    storage_service::setup(&mut device)
}

/// 単一 secret を読み込み、指定された storage object へ保存する。
///
/// 既存 object の上書き可否は secret 入力より前に確定する。
fn run_put_with<B: SecretsBoundary + InteractionBoundary>(
    options: super::PutOptions,
    boundary: &mut B,
) -> Result<()> {
    require_noninteractive_serial(boundary, options.serial, NONINTERACTIVE_SERIAL_ERROR)?;
    require_single_stdin_secret_source(options.stdin, boundary)?;
    let session = SecretSession::start()?;
    let mut device = boundary.open_device(options.serial)?;
    session.run_yubikey_operation(|| {
        storage_service::check_put_preconditions(&mut device, options.name, options.force)
    })?;
    let secret = boundary.read_secret_for_put(options.name, options.stdin, &session)?;
    session.run_yubikey_operation(|| {
        secret.with_secret(|secret| {
            storage_service::put(&mut device, options.name, secret, options.force, &session)
        })
    })
}

/// 指定された secret を device から復号し、stdout へ書き込む。
///
/// stdout が pipe/redirect でない場合は、PIN verification と touch の前に停止する。
fn run_get_with<B: SecretsBoundary + InteractionBoundary>(
    options: super::GetOptions,
    boundary: &mut B,
) -> Result<()> {
    require_noninteractive_serial(boundary, options.serial, NONINTERACTIVE_SERIAL_ERROR)?;
    require_secret_stdout_target(boundary)?;
    let session = SecretSession::start()?;
    let mut device = boundary.open_device(options.serial)?;
    boundary.verify_pin_for_secret_reads(&mut device, &session)?;
    let output_bytes = session.run_yubikey_operation(|| {
        storage_service::get_protected(&mut device, options.name, &session)
    })?;
    output_bytes.with_secret(|bytes| boundary.write_secret_to_stdout(bytes))?;
    Ok(())
}

/// primary 用 enrollment secrets を読み込み、device へ登録して local verify まで実行する。
///
/// storage 衝突確認が終わるまでは enrollment secrets を読み始めない。
fn run_enroll_primary_with<B: SecretsBoundary + InteractionBoundary>(
    options: super::EnrollPrimaryOptions,
    boundary: &mut B,
) -> Result<()> {
    require_noninteractive_serial(boundary, options.serial, NONINTERACTIVE_SERIAL_ERROR)?;
    require_stdin_json_source(boundary, options.stdin_json)?;
    require_noninteractive_option(boundary, options.stdin_json, "--stdin-json")?;
    let session = SecretSession::start()?;
    let mut device = boundary.open_device(options.serial)?;
    session.run_yubikey_operation(|| storage_service::check_setup_preconditions(&mut device))?;
    let summary = {
        if options.stdin_json {
            boundary.verify_pin_for_secret_reads(&mut device, &session)?;
        }
        let secrets = boundary.read_enrollment_secret_set(options.stdin_json, &session)?;
        session.check_interrupted()?;
        if !options.stdin_json {
            boundary.verify_pin_for_secret_reads(&mut device, &session)?;
        }
        let mut summary = session.run_yubikey_operation(|| {
            enroll_without_local_verify(&mut device, YubikeyRole::Primary, &secrets, &session)
        })?;
        verify_local_storage_protected(&mut device, &session)?;
        summary
            .checks
            .insert(CheckName::LocalStorage, CheckStatus::Ok);
        summary
    };
    boundary.write_summary_json(&summary)?;
    Ok(())
}

/// spare 用 enrollment secrets を取得し、別 device へ登録して local verify まで実行する。
///
/// primary から復号する経路では、復号前に spare 候補と serial 制約を確定する。
fn run_enroll_spare_with<B: SecretsBoundary + InteractionBoundary>(
    options: EnrollSpareOptions,
    boundary: &mut B,
) -> Result<()> {
    if !options.stdin_json {
        require_noninteractive_serial(
            boundary,
            options.primary_serial,
            NONINTERACTIVE_PRIMARY_SERIAL_ERROR,
        )?;
    }
    require_noninteractive_serial(
        boundary,
        options.spare_serial,
        NONINTERACTIVE_SPARE_SERIAL_ERROR,
    )?;
    require_stdin_json_source(boundary, options.stdin_json)?;
    let session = SecretSession::start()?;
    let prepared_spare = if options.stdin_json || options.spare_serial.is_some() {
        let mut spare = boundary.open_spare_device(
            options.spare_serial,
            options.primary_serial,
            session.interrupt(),
        )?;
        session.run_yubikey_operation(|| storage_service::check_setup_preconditions(&mut spare))?;
        if options.stdin_json {
            boundary.verify_pin_for_secret_reads(&mut spare, &session)?;
        }
        Some(spare)
    } else {
        None
    };
    let (bootstrap, primary_serial, spare) = if options.stdin_json {
        session.check_interrupted()?;
        (
            boundary.read_enrollment_secret_set(true, &session)?,
            options.primary_serial,
            prepared_spare,
        )
    } else {
        let mut primary = boundary.open_device(options.primary_serial)?;
        let primary_serial = boundary.device_serial(&primary);
        if prepared_spare
            .as_ref()
            .is_some_and(|spare_device| boundary.device_serial(spare_device) == primary_serial)
        {
            bail!("primary and spare YubiKey serial must be different");
        }
        boundary.verify_pin_for_secret_reads(&mut primary, &session)?;
        let secrets = read_protected_bootstrap_from_device(&mut primary, &session)?;
        (secrets, Some(primary_serial), prepared_spare)
    };

    session.check_interrupted()?;
    let mut spare = match spare {
        Some(spare) => spare,
        None => {
            let mut spare = boundary.open_spare_device(
                options.spare_serial,
                primary_serial,
                session.interrupt(),
            )?;
            session
                .run_yubikey_operation(|| storage_service::check_setup_preconditions(&mut spare))?;
            spare
        }
    };

    session.check_interrupted()?;
    if !options.stdin_json {
        boundary.verify_pin_for_secret_reads(&mut spare, &session)?;
    }
    let mut summary = session.run_yubikey_operation(|| {
        enroll_without_local_verify(&mut spare, YubikeyRole::Spare, &bootstrap, &session)
    })?;
    verify_local_storage_protected(&mut spare, &session)?;
    summary
        .checks
        .insert(CheckName::LocalStorage, CheckStatus::Ok);
    drop(bootstrap);
    boundary.write_summary_json(&summary)?;
    Ok(())
}

/// primary device から 登録用の 3 field を復号する。
///
/// 各 field は次の device 操作前に session 所属の保護済み値へ移す。
fn read_protected_bootstrap_from_device<'session, D: SecretDevice>(
    primary: &mut D,
    session: &'session SecretSession,
) -> Result<EnrollmentSecretSet<'session>> {
    let bw_email = session.run_yubikey_operation(|| {
        storage_service::get_protected(primary, SecretName::BwEmail, session)
    })?;
    let bw_password = session.run_yubikey_operation(|| {
        storage_service::get_protected(primary, SecretName::BwPassword, session)
    })?;
    let bws_access_token = session.run_yubikey_operation(|| {
        storage_service::get_protected(primary, SecretName::BwsAccessToken, session)
    })?;
    Ok(EnrollmentSecretSet::new(
        bw_email,
        bw_password,
        bws_access_token,
    ))
}

/// 登録の永続書き込みを行い、local verify 前の summary を返す。
///
/// 3 field の空チェックを完了してから PIV key / manifest 作成へ進む。
fn enroll_without_local_verify<D: SecretDevice>(
    device: &mut D,
    role: YubikeyRole,
    secrets: &EnrollmentSecretSet<'_>,
    session: &SecretSession,
) -> Result<EnrollSummary> {
    secrets.bw_email.with_secret(|secret| {
        if secret.is_empty() {
            bail!("{} must not be empty", SecretName::BwEmail);
        }
        Ok(())
    })?;
    secrets.bw_password.with_secret(|secret| {
        if secret.is_empty() {
            bail!("{} must not be empty", SecretName::BwPassword);
        }
        Ok(())
    })?;
    secrets.bws_access_token.with_secret(|secret| {
        if secret.is_empty() {
            bail!("{} must not be empty", SecretName::BwsAccessToken);
        }
        Ok(())
    })?;

    storage_service::setup(device)?;
    session.check_interrupted()?;
    secrets.bw_email.with_secret(|secret| {
        storage_service::put(device, SecretName::BwEmail, secret, false, session)
    })?;
    session.check_interrupted()?;
    secrets.bw_password.with_secret(|secret| {
        storage_service::put(device, SecretName::BwPassword, secret, false, session)
    })?;
    session.check_interrupted()?;
    secrets.bws_access_token.with_secret(|secret| {
        storage_service::put(device, SecretName::BwsAccessToken, secret, false, session)
    })?;
    Ok(storage_service::enroll_summary(device.serial(), role))
}

/// BWS access token を読み込み、1 本または複数本の device へ反映する。
///
/// 複数更新では 1 回読んだ token を session 内で再利用する。
fn run_rotate_bws_token_with<B: SecretsBoundary + InteractionBoundary>(
    options: super::RotateBwsTokenOptions,
    boundary: &mut B,
) -> Result<()> {
    require_single_stdin_secret_source(options.stdin, boundary)?;
    let session = SecretSession::start()?;

    if let Some(serial) = options.serial {
        let mut device = boundary.open_device(Some(serial))?;
        prepare_bws_token_rotation_device(boundary, &mut device, &session)?;
        let token =
            boundary.read_secret_for_put(SecretName::BwsAccessToken, options.stdin, &session)?;
        let serial = boundary.device_serial(&device);
        let rotation = rotate_bws_token_on_device(&mut device, serial, &token, &session)?;
        drop(token);
        boundary.write_summary_json(&rotation.summary)?;
        return rotation.result;
    }

    require_noninteractive_serial(boundary, None, NONINTERACTIVE_SERIAL_ERROR)?;
    require_noninteractive_option(boundary, options.stdin, "--stdin")?;
    let mut device = boundary.open_device(None)?;
    prepare_bws_token_rotation_device(boundary, &mut device, &session)?;
    let token =
        boundary.read_secret_for_put(SecretName::BwsAccessToken, options.stdin, &session)?;
    let mut updated_serials = BTreeSet::from([boundary.device_serial(&device)]);
    let first_serial = boundary.device_serial(&device);
    let first_rotation = rotate_bws_token_on_device(&mut device, first_serial, &token, &session)?;
    let mut summaries = vec![first_rotation.summary];
    if let Err(err) = first_rotation.result {
        drop(token);
        write_partial_rotate_bws_token_summary(boundary, &summaries)?;
        return Err(err);
    }
    drop(device);

    let remaining_result = (|| -> Result<()> {
        while session.run_yubikey_operation(|| {
            boundary.prompt_yes_no("Update another YubiKey? [y/N] ", session.interrupt())
        })? {
            session.check_interrupted()?;
            let mut device = boundary.open_device(None)?;
            session.check_interrupted()?;
            if !updated_serials.insert(boundary.device_serial(&device)) {
                bail!("selected YubiKey was already updated");
            }
            prepare_bws_token_rotation_device(boundary, &mut device, &session)?;
            let serial = boundary.device_serial(&device);
            let rotation = rotate_bws_token_on_device(&mut device, serial, &token, &session)?;
            summaries.push(rotation.summary);
            rotation.result?;
        }
        Ok(())
    })();

    if let Err(err) = remaining_result {
        drop(token);
        write_partial_rotate_bws_token_summary(boundary, &summaries)?;
        return Err(err);
    }

    drop(token);
    boundary.write_summary_json(&summaries)?;
    Ok(())
}

/// BWS token rotation の対象 device を開き、更新前条件を確認する。
///
/// token 入力前に既存 secrets の復号確認と management auth を済ませる。
fn prepare_bws_token_rotation_device<B: SecretsBoundary + InteractionBoundary>(
    boundary: &mut B,
    device: &mut B::Device,
    session: &SecretSession,
) -> Result<()> {
    boundary.verify_pin_for_secret_reads(device, session)?;
    check_rotate_preconditions_protected(boundary, device, session)
}

/// 1 本の device へ BWS access token を書き込み、local verify を実行する。
///
/// token の平文借用範囲は storage 書き込み呼び出し中に限定する。
fn rotate_bws_token_on_device<D: SecretDevice>(
    device: &mut D,
    serial: u32,
    token: &ProtectedSecret<'_>,
    session: &SecretSession,
) -> Result<RotateBwsTokenResult> {
    session.run_yubikey_operation(|| {
        token.with_secret(|token| storage_service::replace_bws_token(device, token, session))
    })?;
    match verify_local_storage_protected(device, session) {
        Ok(summary) => Ok(RotateBwsTokenResult {
            summary,
            result: Ok(()),
        }),
        Err(err) => Ok(RotateBwsTokenResult {
            summary: failed_local_storage_summary(serial),
            result: Err(err),
        }),
    }
}

struct RotateBwsTokenResult {
    summary: VerifySummary,
    result: Result<()>,
}

#[derive(serde::Serialize)]
struct PartialRotateBwsTokenSummary<'a> {
    updated: &'a [VerifySummary],
}

/// rotation 済み device の summary を部分成功 JSON として stdout へ出力する。
///
/// 途中失敗時に、利用者が再実行対象を判別できる情報を残す。
fn write_partial_rotate_bws_token_summary<B: SecretsBoundary + InteractionBoundary>(
    boundary: &mut B,
    summaries: &[VerifySummary],
) -> Result<()> {
    if summaries.is_empty() {
        return Ok(());
    }

    let partial = PartialRotateBwsTokenSummary { updated: summaries };
    boundary.write_summary_json(&partial)?;
    Ok(())
}

fn failed_local_storage_summary(serial: u32) -> VerifySummary {
    VerifySummary {
        serial,
        checks: [
            (CheckName::LocalStorage, CheckStatus::Failed),
            (CheckName::Bws, CheckStatus::Skipped),
            (CheckName::BwLogin, CheckStatus::Skipped),
        ]
        .into_iter()
        .collect(),
    }
}

/// YubiKey local storage の verify を実行し、summary JSON を出力する。
///
/// 未実装の外部 service check は device touch 前に拒否する。
fn run_verify_yubikey_with<B: SecretsBoundary + InteractionBoundary>(
    options: VerifyYubikeyOptions,
    boundary: &mut B,
) -> Result<()> {
    require_noninteractive_serial(boundary, options.serial, NONINTERACTIVE_SERIAL_ERROR)?;
    if options.all && !options.check.is_empty() {
        bail!("--all and --check cannot be used together");
    }
    let session = SecretSession::start()?;
    let mut device = boundary.open_device(options.serial)?;
    boundary.verify_pin_for_secret_reads(&mut device, &session)?;
    let mut summary = verify_local_storage_protected(&mut device, &session)?;
    let requested = requested_external_checks(&options);
    if !requested.is_empty() {
        for check in &requested {
            summary.checks.insert(*check, CheckStatus::Failed);
        }
        boundary.write_summary_json(&summary)?;
        let requested_names = requested
            .iter()
            .map(|check| match check {
                CheckName::Bws => "bws",
                CheckName::BwLogin => "bw-login",
                _ => unreachable!("requested_external_checks returns only external checks"),
            })
            .collect::<Vec<_>>()
            .join(", ");
        bail!("external checks are not implemented yet: {requested_names}");
    }

    boundary.write_summary_json(&summary)?;
    Ok(())
}

fn requested_external_checks(options: &VerifyYubikeyOptions) -> Vec<CheckName> {
    if options.all {
        return vec![CheckName::Bws, CheckName::BwLogin];
    }
    options
        .check
        .iter()
        .map(|check| match check {
            VerifyCheck::Bws => CheckName::Bws,
            VerifyCheck::BwLogin => CheckName::BwLogin,
        })
        .collect()
}

/// device 上の local storage secrets を復号し、空でないことを確認する。
///
/// 復号結果は空判定前に session の保護境界へ移す。
fn verify_local_storage_protected<D: SecretDevice>(
    device: &mut D,
    session: &SecretSession,
) -> Result<VerifySummary> {
    for name in SecretName::iter() {
        let secret = session
            .run_yubikey_operation(|| storage_service::get_protected(device, name, session))?;
        secret.with_secret(|secret| {
            if secret.is_empty() {
                bail!("{} stored on this YubiKey is empty", name);
            }
            Ok(())
        })?;
    }

    Ok(VerifySummary {
        serial: device.serial(),
        checks: [
            (CheckName::LocalStorage, CheckStatus::Ok),
            (CheckName::Bws, CheckStatus::Skipped),
            (CheckName::BwLogin, CheckStatus::Skipped),
        ]
        .into_iter()
        .collect(),
    })
}

/// rotation の書き込み前条件として local verify と management auth を確認する。
///
/// 確認は token 入力前に現在の保護境界内で実行する。
fn check_rotate_preconditions_protected<B: SecretsBoundary + InteractionBoundary>(
    boundary: &mut B,
    device: &mut B::Device,
    session: &SecretSession,
) -> Result<()> {
    verify_local_storage_protected(device, session)?;
    boundary.check_management_auth_preconditions(device, session)
}

/// device が要求する場合に PIN を入力境界から読み取り、PIV session を検証する。
///
/// PIN 入力順序は application が所有し、device は検証済み状態かどうかだけを公開する。
/// 単一 secret を stdin から読む command の入力契約を確認する。
///
/// 非対話では `--stdin` を必須にし、TTY stdin では hidden prompt と混同しないよう拒否する。
fn require_single_stdin_secret_source<B: SecretsBoundary + InteractionBoundary>(
    stdin: bool,
    boundary: &B,
) -> Result<()> {
    if stdin {
        if boundary.stdin_is_terminal() {
            bail!("--stdin requires pipe or redirect input");
        }
        return Ok(());
    }

    require_noninteractive_option(boundary, false, "--stdin")
}

/// 非対話実行で対象 serial を省略していないかを、device 操作前に確認する。
fn require_noninteractive_serial<B: SecretsBoundary + InteractionBoundary>(
    boundary: &B,
    serial: Option<u32>,
    error_message: &'static str,
) -> Result<()> {
    if serial.is_none() && !boundary.stdin_is_terminal() {
        bail!(error_message);
    }
    Ok(())
}

/// 非対話実行で必須 option を欠いていないかを、入力消費前に確認する。
fn require_noninteractive_option<B: SecretsBoundary + InteractionBoundary>(
    boundary: &B,
    enabled: bool,
    option_name: &'static str,
) -> Result<()> {
    if !enabled && !boundary.stdin_is_terminal() {
        bail!("pass {option_name} in non-interactive use");
    }
    Ok(())
}

/// 平文 secret を stdout へ出す経路が端末を向いていないことを確認する。
fn require_secret_stdout_target<B: SecretsBoundary + InteractionBoundary>(
    boundary: &B,
) -> Result<()> {
    boundary.ensure_secret_stdout_not_terminal()
}

/// `--stdin-json` は平文 secret を端末へ echo しないよう、pipe/redirect のみ許可する。
fn require_stdin_json_source<B: SecretsBoundary + InteractionBoundary>(
    boundary: &B,
    stdin_json: bool,
) -> Result<()> {
    if stdin_json && boundary.stdin_is_terminal() {
        bail!(STDIN_JSON_TTY_ERROR);
    }
    Ok(())
}
