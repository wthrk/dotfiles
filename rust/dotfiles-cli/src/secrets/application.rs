//! `dotfiles secrets` の application 層。
//!
//! この層は command ごとの use case と外部境界の順序を所有する。secret を読む前に
//! device と非対話条件を確定し、平文 secret は `SecretSession` に紐づく保護済み値として
//! domain の保存操作へ渡す。

mod real_boundary;
mod storage_service;
#[cfg(test)]
mod storage_service_tests;

use std::collections::BTreeSet;

use super::{
    EnrollSpareOptions, SecretsCommand, SecretsOptions, VerifyCheck, VerifyYubikeyOptions,
    YubikeyCommand, YubikeyOptions, adapters,
    adapters::input::{
        MAX_BOOTSTRAP_JSON_LEN, MAX_SINGLE_STDIN_SECRET_LEN, read_hidden_secret,
        read_protected_enrollment_secret_set, read_protected_stdin_secret,
        read_visible_secret_line, write_secret_to_stdout,
    },
    domain::{self, SecretDevice, SecretName},
    ports::{EnrollmentSecretSet, SecretsBoundary},
    support::protection::{ProtectedSecret, SecretSession},
};
use crate::Result;
use anyhow::bail;

const NONINTERACTIVE_SERIAL_ERROR: &str = "pass --serial in non-interactive use";
const NONINTERACTIVE_PRIMARY_SERIAL_ERROR: &str = "pass --primary-serial in non-interactive use";
const NONINTERACTIVE_SPARE_SERIAL_ERROR: &str = "pass --spare-serial in non-interactive use";
const STDIN_JSON_TTY_ERROR: &str = "--stdin-json requires pipe or redirect input";

/// 指定された device backend を使って parse 済み options の use case を開始する。
///
/// device backend は実機と stub の差分だけを持ち、secret 入力や stdout 判定は同じ境界を通す。
pub(super) fn run(options: SecretsOptions, backend: adapters::DeviceBackend) -> Result<()> {
    let mut boundary = real_boundary::RealSecretsBoundary { backend };
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

/// `setup` 用の device を開き、PIV object setup を実行する。
///
/// PIV 領域の衝突検出は domain 層に委ねる。
fn run_setup_with<B: SecretsBoundary>(
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
fn run_put_with<B: SecretsBoundary>(options: super::PutOptions, boundary: &mut B) -> Result<()> {
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
fn run_get_with<B: SecretsBoundary>(options: super::GetOptions, boundary: &mut B) -> Result<()> {
    require_noninteractive_serial(boundary, options.serial, NONINTERACTIVE_SERIAL_ERROR)?;
    require_secret_stdout_target(boundary)?;
    let session = SecretSession::start()?;
    let mut device = boundary.open_device(options.serial)?;
    verify_pin_for_secret_reads(boundary, &mut device, &session)?;
    let output_bytes = session.run_yubikey_operation(|| {
        storage_service::get_protected(&mut device, options.name, &session)
    })?;
    output_bytes.with_secret(write_secret_to_stdout)?;
    Ok(())
}

/// primary 用 enrollment secrets を読み込み、device へ登録して local verify まで実行する。
///
/// storage 衝突確認が終わるまでは enrollment secrets を読み始めない。
fn run_enroll_primary_with<B: SecretsBoundary>(
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
            verify_pin_for_secret_reads(boundary, &mut device, &session)?;
        }
        let secrets = boundary.read_enrollment_secret_set(options.stdin_json, &session)?;
        session.check_interrupted()?;
        if !options.stdin_json {
            verify_pin_for_secret_reads(boundary, &mut device, &session)?;
        }
        let mut summary = session.run_yubikey_operation(|| {
            enroll_without_local_verify(
                &mut device,
                domain::YubikeyRole::Primary,
                &secrets,
                &session,
            )
        })?;
        verify_local_storage_protected(&mut device, &session)?;
        summary
            .checks
            .insert(domain::CheckName::LocalStorage, domain::CheckStatus::Ok);
        summary
    };
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

/// `put` 用の単一 secret を prompt または stdin から読み込む。
///
/// 読み込んだ直後に session 所属の保護済み値へ移す。
pub(crate) fn read_protected_secret_for_put(
    name: SecretName,
    stdin: bool,
    memory: &SecretSession,
) -> Result<ProtectedSecret<'_>> {
    if stdin {
        read_protected_stdin_secret(MAX_SINGLE_STDIN_SECRET_LEN, memory)
    } else {
        read_hidden_secret(&format!("{}: ", name), MAX_SINGLE_STDIN_SECRET_LEN, memory)
    }
}

/// 登録用の 3 field を prompt または stdin JSON から読み込む。
///
/// field ごとの保護境界を同じ session にそろえてから登録用 model にする。
fn read_enrollment_secret_set_from_user(
    stdin_json: bool,
    memory: &SecretSession,
) -> Result<EnrollmentSecretSet<'_>> {
    if stdin_json {
        return read_protected_enrollment_secret_set(
            std::io::stdin(),
            MAX_BOOTSTRAP_JSON_LEN,
            MAX_SINGLE_STDIN_SECRET_LEN,
            memory,
        );
    }

    let bw_email = read_visible_secret_line("bw-email: ", MAX_SINGLE_STDIN_SECRET_LEN, memory)?;
    let bw_password = read_protected_secret_for_put(SecretName::BwPassword, false, memory)?;
    let bws_access_token =
        read_protected_secret_for_put(SecretName::BwsAccessToken, false, memory)?;

    Ok(EnrollmentSecretSet::new(
        bw_email,
        bw_password,
        bws_access_token,
    ))
}

/// spare 用 enrollment secrets を取得し、別 device へ登録して local verify まで実行する。
///
/// primary から復号する経路では、復号前に spare 候補と serial 制約を確定する。
fn run_enroll_spare_with<B: SecretsBoundary>(
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
            verify_pin_for_secret_reads(boundary, &mut spare, &session)?;
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
        let primary_serial = primary.serial();
        if prepared_spare
            .as_ref()
            .is_some_and(|spare_device| spare_device.serial() == primary_serial)
        {
            bail!("primary and spare YubiKey serial must be different");
        }
        verify_pin_for_secret_reads(boundary, &mut primary, &session)?;
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
        verify_pin_for_secret_reads(boundary, &mut spare, &session)?;
    }
    let mut summary = session.run_yubikey_operation(|| {
        enroll_without_local_verify(&mut spare, domain::YubikeyRole::Spare, &bootstrap, &session)
    })?;
    verify_local_storage_protected(&mut spare, &session)?;
    summary
        .checks
        .insert(domain::CheckName::LocalStorage, domain::CheckStatus::Ok);
    drop(bootstrap);
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

/// primary device から 登録用の 3 field を復号する。
///
/// 各 field は次の device 操作前に session 所属の保護済み値へ移す。
fn read_protected_bootstrap_from_device<'session, D: domain::SecretDevice>(
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
fn enroll_without_local_verify<D: domain::SecretDevice>(
    device: &mut D,
    role: domain::YubikeyRole,
    secrets: &EnrollmentSecretSet<'_>,
    session: &SecretSession,
) -> Result<domain::EnrollSummary> {
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
fn run_rotate_bws_token_with<B: SecretsBoundary>(
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
        let rotation = rotate_bws_token_on_device(&mut device, &token, &session)?;
        drop(token);
        println!("{}", serde_json::to_string_pretty(&rotation.summary)?);
        return rotation.result;
    }

    require_noninteractive_serial(boundary, None, NONINTERACTIVE_SERIAL_ERROR)?;
    require_noninteractive_option(boundary, options.stdin, "--stdin")?;
    let mut device = boundary.open_device(None)?;
    prepare_bws_token_rotation_device(boundary, &mut device, &session)?;
    let token =
        boundary.read_secret_for_put(SecretName::BwsAccessToken, options.stdin, &session)?;
    let mut updated_serials = BTreeSet::from([device.serial()]);
    let first_rotation = rotate_bws_token_on_device(&mut device, &token, &session)?;
    let mut summaries = vec![first_rotation.summary];
    if let Err(err) = first_rotation.result {
        drop(token);
        write_partial_rotate_bws_token_summary(&summaries)?;
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
            if !updated_serials.insert(device.serial()) {
                bail!("selected YubiKey was already updated");
            }
            prepare_bws_token_rotation_device(boundary, &mut device, &session)?;
            let rotation = rotate_bws_token_on_device(&mut device, &token, &session)?;
            summaries.push(rotation.summary);
            rotation.result?;
        }
        Ok(())
    })();

    if let Err(err) = remaining_result {
        drop(token);
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
    device: &mut B::Device,
    session: &SecretSession,
) -> Result<()> {
    verify_pin_for_secret_reads(boundary, device, session)?;
    check_rotate_preconditions_protected(device, session)
}

/// 1 本の device へ BWS access token を書き込み、local verify を実行する。
///
/// token の平文借用範囲は storage 書き込み呼び出し中に限定する。
fn rotate_bws_token_on_device<D: domain::SecretDevice>(
    device: &mut D,
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
            summary: failed_local_storage_summary(device.serial()),
            result: Err(err),
        }),
    }
}

struct RotateBwsTokenResult {
    summary: domain::VerifySummary,
    result: Result<()>,
}

#[derive(serde::Serialize)]
struct PartialRotateBwsTokenSummary<'a> {
    updated: &'a [domain::VerifySummary],
}

/// rotation 済み device の summary を部分成功 JSON として stdout へ出力する。
///
/// 途中失敗時に、利用者が再実行対象を判別できる情報を残す。
fn write_partial_rotate_bws_token_summary(summaries: &[domain::VerifySummary]) -> Result<()> {
    if summaries.is_empty() {
        return Ok(());
    }

    let partial = PartialRotateBwsTokenSummary { updated: summaries };
    println!("{}", serde_json::to_string_pretty(&partial)?);
    Ok(())
}

fn failed_local_storage_summary(serial: u32) -> domain::VerifySummary {
    domain::VerifySummary {
        serial,
        checks: [
            (domain::CheckName::LocalStorage, domain::CheckStatus::Failed),
            (domain::CheckName::Bws, domain::CheckStatus::Skipped),
            (domain::CheckName::BwLogin, domain::CheckStatus::Skipped),
        ]
        .into_iter()
        .collect(),
    }
}

/// YubiKey local storage の verify を実行し、summary JSON を出力する。
///
/// 未実装の外部 service check は device touch 前に拒否する。
fn run_verify_yubikey_with<B: SecretsBoundary>(
    options: VerifyYubikeyOptions,
    boundary: &mut B,
) -> Result<()> {
    require_noninteractive_serial(boundary, options.serial, NONINTERACTIVE_SERIAL_ERROR)?;
    if options.all && !options.check.is_empty() {
        bail!("--all and --check cannot be used together");
    }
    let session = SecretSession::start()?;
    let mut device = boundary.open_device(options.serial)?;
    verify_pin_for_secret_reads(boundary, &mut device, &session)?;
    let mut summary = verify_local_storage_protected(&mut device, &session)?;
    let requested = requested_external_checks(&options);
    if !requested.is_empty() {
        for check in &requested {
            summary.checks.insert(*check, domain::CheckStatus::Failed);
        }
        println!("{}", serde_json::to_string_pretty(&summary)?);
        let requested_names = requested
            .iter()
            .map(|check| match check {
                domain::CheckName::Bws => "bws",
                domain::CheckName::BwLogin => "bw-login",
                _ => unreachable!("requested_external_checks returns only external checks"),
            })
            .collect::<Vec<_>>()
            .join(", ");
        bail!("external checks are not implemented yet: {requested_names}");
    }

    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

fn requested_external_checks(options: &VerifyYubikeyOptions) -> Vec<domain::CheckName> {
    if options.all {
        return vec![domain::CheckName::Bws, domain::CheckName::BwLogin];
    }
    options
        .check
        .iter()
        .map(|check| match check {
            VerifyCheck::Bws => domain::CheckName::Bws,
            VerifyCheck::BwLogin => domain::CheckName::BwLogin,
        })
        .collect()
}

/// device 上の local storage secrets を復号し、空でないことを確認する。
///
/// 復号結果は空判定前に session の保護境界へ移す。
fn verify_local_storage_protected<D: domain::SecretDevice>(
    device: &mut D,
    session: &SecretSession,
) -> Result<domain::VerifySummary> {
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

    Ok(domain::VerifySummary {
        serial: device.serial(),
        checks: [
            (domain::CheckName::LocalStorage, domain::CheckStatus::Ok),
            (domain::CheckName::Bws, domain::CheckStatus::Skipped),
            (domain::CheckName::BwLogin, domain::CheckStatus::Skipped),
        ]
        .into_iter()
        .collect(),
    })
}

/// rotation の書き込み前条件として local verify と management auth を確認する。
///
/// 確認は token 入力前に現在の保護境界内で実行する。
fn check_rotate_preconditions_protected<D: domain::SecretDevice>(
    device: &mut D,
    session: &SecretSession,
) -> Result<()> {
    verify_local_storage_protected(device, session)?;
    session.run_yubikey_operation(|| device.check_management_auth_preconditions())
}

/// device が要求する場合に PIN を入力境界から読み取り、PIV session を検証する。
///
/// PIN 入力順序は application が所有し、device は検証済み状態かどうかだけを公開する。
fn verify_pin_for_secret_reads<B: SecretsBoundary>(
    boundary: &mut B,
    device: &mut B::Device,
    session: &SecretSession,
) -> Result<()> {
    if !device.requires_pin_input() {
        return Ok(());
    }

    let pin = boundary.read_yubikey_pin(session)?;
    pin.with_secret(|pin| session.run_yubikey_operation(|| device.verify_pin(pin)))
}

/// 単一 secret を stdin から読む command の入力契約を確認する。
///
/// 非対話では `--stdin` を必須にし、TTY stdin では hidden prompt と混同しないよう拒否する。
fn require_single_stdin_secret_source<B: SecretsBoundary>(stdin: bool, boundary: &B) -> Result<()> {
    if stdin {
        if boundary.stdin_is_terminal() {
            bail!("--stdin requires pipe or redirect input");
        }
        return Ok(());
    }

    require_noninteractive_option(boundary, false, "--stdin")
}

/// 非対話実行で対象 serial を省略していないかを、device 操作前に確認する。
fn require_noninteractive_serial<B: SecretsBoundary>(
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
fn require_noninteractive_option<B: SecretsBoundary>(
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
fn require_secret_stdout_target<B: SecretsBoundary>(boundary: &B) -> Result<()> {
    if boundary.stdout_is_terminal() {
        super::adapters::input::reject_secret_stdout_terminal()?;
    }
    Ok(())
}

/// `--stdin-json` は平文 secret を端末へ echo しないよう、pipe/redirect のみ許可する。
fn require_stdin_json_source<B: SecretsBoundary>(boundary: &B, stdin_json: bool) -> Result<()> {
    if stdin_json && boundary.stdin_is_terminal() {
        bail!(STDIN_JSON_TTY_ERROR);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};
    use std::{
        cell::RefCell,
        collections::{BTreeMap, VecDeque},
        rc::Rc,
    };

    use super::*;
    use crate::secrets::support::protection::{InterruptGuard, ProtectedInputBuffer};

    struct FakeBoundary {
        devices: VecDeque<FakeDevice>,
        prompts: VecDeque<bool>,
        stdin_terminal: bool,
        stdout_terminal: bool,
        enrollment_read_calls: usize,
    }

    impl FakeBoundary {
        fn new(devices: Vec<FakeDevice>) -> Self {
            Self {
                devices: devices.into(),
                prompts: VecDeque::new(),
                stdin_terminal: true,
                stdout_terminal: false,
                enrollment_read_calls: 0,
            }
        }

        fn with_prompts(mut self, prompts: Vec<bool>) -> Self {
            self.prompts = prompts.into();
            self
        }

        fn with_stdin_terminal(mut self, stdin_terminal: bool) -> Self {
            self.stdin_terminal = stdin_terminal;
            self
        }
    }

    impl SecretsBoundary for FakeBoundary {
        type Device = FakeDevice;

        fn stdin_is_terminal(&self) -> bool {
            self.stdin_terminal
        }

        fn stdout_is_terminal(&self) -> bool {
            self.stdout_terminal
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
            primary_serial: Option<u32>,
            _interrupt: &InterruptGuard,
        ) -> Result<Self::Device> {
            let device = self.open_device(spare_serial)?;
            if primary_serial == Some(device.serial) {
                bail!("primary and spare YubiKey serial must be different");
            }
            Ok(device)
        }

        fn read_enrollment_secret_set<'session>(
            &mut self,
            _stdin_json: bool,
            memory: &'session SecretSession,
        ) -> Result<EnrollmentSecretSet<'session>> {
            self.enrollment_read_calls += 1;
            protected_enrollment_secret_set(memory)
        }

        fn read_secret_for_put<'session>(
            &mut self,
            _name: SecretName,
            _stdin: bool,
            memory: &'session SecretSession,
        ) -> Result<ProtectedSecret<'session>> {
            protected_test_secret(b"rotated-token", memory)
        }

        fn read_yubikey_pin<'session>(
            &mut self,
            memory: &'session SecretSession,
        ) -> Result<ProtectedSecret<'session>> {
            protected_test_secret(b"123456", memory)
        }

        fn prompt_yes_no(&mut self, _prompt: &str, _interrupt: &InterruptGuard) -> Result<bool> {
            self.prompts
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("fake prompt queue is empty"))
        }
    }

    struct FakeDevice {
        serial: u32,
        state: Rc<RefCell<FakeDeviceState>>,
        pin_error: Option<&'static str>,
    }

    #[derive(Default)]
    struct FakeDeviceState {
        key_exists: bool,
        objects: BTreeMap<domain::PivObjectId, Vec<u8>>,
    }

    impl FakeDevice {
        fn fresh(serial: u32) -> Self {
            Self {
                serial,
                state: Rc::new(RefCell::new(FakeDeviceState::default())),
                pin_error: None,
            }
        }

        fn fresh_with_state(serial: u32) -> (Self, Rc<RefCell<FakeDeviceState>>) {
            let state = Rc::new(RefCell::new(FakeDeviceState::default()));
            (
                Self {
                    serial,
                    state: Rc::clone(&state),
                    pin_error: None,
                },
                state,
            )
        }

        fn with_pin_error(mut self, pin_error: &'static str) -> Self {
            self.pin_error = Some(pin_error);
            self
        }

        fn provisioned(serial: u32) -> Result<Self> {
            let mut device = Self::fresh(serial);
            let session = SecretSession::start()?;
            let secrets = protected_enrollment_secret_set(&session)?;
            enroll_without_local_verify(
                &mut device,
                domain::YubikeyRole::Primary,
                &secrets,
                &session,
            )?;
            Ok(device)
        }
    }

    impl domain::SecretDevice for FakeDevice {
        fn serial(&self) -> u32 {
            self.serial
        }

        fn key_exists(&mut self) -> Result<bool> {
            Ok(self.state.borrow().key_exists)
        }

        fn check_key_generation_preconditions(&mut self) -> Result<()> {
            Ok(())
        }

        fn check_management_auth_preconditions(&mut self) -> Result<()> {
            Ok(())
        }

        fn generate_key(&mut self) -> Result<()> {
            self.state.borrow_mut().key_exists = true;
            Ok(())
        }

        fn read_object(&mut self, object_id: domain::PivObjectId) -> Result<Option<Vec<u8>>> {
            Ok(self.state.borrow().objects.get(&object_id).cloned())
        }

        fn write_object(&mut self, object_id: domain::PivObjectId, value: &mut [u8]) -> Result<()> {
            self.state
                .borrow_mut()
                .objects
                .insert(object_id, value.to_vec());
            Ok(())
        }

        fn wrap_key(&mut self, key: &[u8]) -> Result<Vec<u8>> {
            Ok(key.iter().map(|byte| byte ^ 0xa5).collect())
        }

        fn verify_pin(&mut self, _pin: &[u8]) -> Result<()> {
            if let Some(pin_error) = self.pin_error {
                bail!(pin_error);
            }
            Ok(())
        }

        fn requires_pin_input(&self) -> bool {
            true
        }

        fn write_unwrapped_key(
            &mut self,
            wrapped_key: &[u8],
            output: &mut impl Write,
        ) -> Result<()> {
            output.write_all(&self.wrap_key(wrapped_key)?)?;
            Ok(())
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

    #[test]
    fn partial_rotate_summary_serializes_updated_entries() -> Result<()> {
        let summaries = vec![failed_local_storage_summary(42)];
        let value = serde_json::to_value(PartialRotateBwsTokenSummary {
            updated: &summaries,
        })?;
        let updated = value
            .get("updated")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("updated field is missing"))?;
        assert_eq!(updated.len(), 1);
        let serial = updated[0]
            .get("serial")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| anyhow::anyhow!("serial field is missing"))?;
        assert_eq!(serial, 42);
        let checks = updated[0]
            .get("checks")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| anyhow::anyhow!("checks field is missing"))?;
        assert_eq!(
            checks
                .get("local_storage")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("local_storage check is missing"))?,
            "failed"
        );
        Ok(())
    }

    #[test]
    fn partial_rotate_summary_skips_output_when_empty() -> Result<()> {
        write_partial_rotate_bws_token_summary(&[])?;
        Ok(())
    }

    #[test]
    fn put_rejects_noninteractive_without_serial_before_device_open() -> Result<()> {
        let mut boundary = FakeBoundary::new(vec![]).with_stdin_terminal(false);
        let options = super::super::PutOptions {
            serial: None,
            name: SecretName::BwsAccessToken,
            stdin: true,
            force: false,
        };
        let err = run_put_with(options, &mut boundary)
            .err()
            .ok_or_else(|| anyhow::anyhow!("put unexpectedly succeeded"))?;
        assert_eq!(err.to_string(), "pass --serial in non-interactive use");
        Ok(())
    }

    #[test]
    fn setup_rejects_noninteractive_without_serial_before_device_open() -> Result<()> {
        let mut boundary = FakeBoundary::new(vec![]).with_stdin_terminal(false);
        let options = super::super::SerialOptions { serial: None };
        let err = run_setup_with(options, &mut boundary)
            .err()
            .ok_or_else(|| anyhow::anyhow!("setup unexpectedly succeeded"))?;
        assert_eq!(err.to_string(), "pass --serial in non-interactive use");
        Ok(())
    }

    #[test]
    fn put_rejects_noninteractive_without_stdin_option() -> Result<()> {
        let mut boundary =
            FakeBoundary::new(vec![FakeDevice::provisioned(10)?]).with_stdin_terminal(false);
        let options = super::super::PutOptions {
            serial: Some(10),
            name: SecretName::BwsAccessToken,
            stdin: false,
            force: true,
        };
        let err = run_put_with(options, &mut boundary)
            .err()
            .ok_or_else(|| anyhow::anyhow!("put unexpectedly accepted missing --stdin"))?;
        assert_eq!(err.to_string(), "pass --stdin in non-interactive use");
        Ok(())
    }

    #[test]
    fn put_rejects_tty_stdin_before_device_open() -> Result<()> {
        let mut boundary = FakeBoundary::new(vec![]);
        let options = super::super::PutOptions {
            serial: Some(10),
            name: SecretName::BwsAccessToken,
            stdin: true,
            force: false,
        };
        let err = run_put_with(options, &mut boundary)
            .err()
            .ok_or_else(|| anyhow::anyhow!("put unexpectedly accepted tty stdin"))?;
        assert_eq!(err.to_string(), "--stdin requires pipe or redirect input");
        Ok(())
    }

    #[test]
    fn rotate_bws_token_rejects_noninteractive_without_serial() -> Result<()> {
        let mut boundary = FakeBoundary::new(vec![]).with_stdin_terminal(false);
        let options = super::super::RotateBwsTokenOptions {
            serial: None,
            stdin: true,
        };
        let err = run_rotate_bws_token_with(options, &mut boundary)
            .err()
            .ok_or_else(|| anyhow::anyhow!("rotate-bws-token unexpectedly succeeded"))?;
        assert_eq!(err.to_string(), "pass --serial in non-interactive use");
        Ok(())
    }

    #[test]
    fn enroll_primary_rejects_tty_stdin_json_before_device_open() -> Result<()> {
        let mut boundary = FakeBoundary::new(vec![]);
        let options = super::super::EnrollPrimaryOptions {
            serial: Some(10),
            stdin_json: true,
        };
        let err = run_enroll_primary_with(options, &mut boundary)
            .err()
            .ok_or_else(|| {
                anyhow::anyhow!("enroll-primary unexpectedly accepted tty stdin-json")
            })?;
        assert_eq!(
            err.to_string(),
            "--stdin-json requires pipe or redirect input"
        );
        Ok(())
    }

    #[test]
    fn enroll_spare_rejects_tty_stdin_json_before_device_open() -> Result<()> {
        let mut boundary = FakeBoundary::new(vec![]);
        let options = EnrollSpareOptions {
            primary_serial: Some(10),
            spare_serial: Some(20),
            stdin_json: true,
        };
        let err = run_enroll_spare_with(options, &mut boundary)
            .err()
            .ok_or_else(|| anyhow::anyhow!("enroll-spare unexpectedly accepted tty stdin-json"))?;
        assert_eq!(
            err.to_string(),
            "--stdin-json requires pipe or redirect input"
        );
        Ok(())
    }

    #[test]
    fn enroll_primary_stdin_json_stops_before_secret_read_when_pin_verification_fails() -> Result<()>
    {
        let mut boundary = FakeBoundary::new(vec![
            FakeDevice::fresh(10).with_pin_error("pin verification failed"),
        ])
        .with_stdin_terminal(false);
        let options = super::super::EnrollPrimaryOptions {
            serial: Some(10),
            stdin_json: true,
        };

        let err = run_enroll_primary_with(options, &mut boundary)
            .err()
            .ok_or_else(|| anyhow::anyhow!("enroll-primary unexpectedly succeeded"))?;

        assert_eq!(err.to_string(), "pin verification failed");
        assert_eq!(boundary.enrollment_read_calls, 0);
        Ok(())
    }

    #[test]
    fn enroll_spare_stdin_json_stops_before_secret_read_when_pin_verification_fails() -> Result<()>
    {
        let mut boundary = FakeBoundary::new(vec![
            FakeDevice::fresh(20).with_pin_error("pin verification failed"),
        ])
        .with_stdin_terminal(false);
        let options = EnrollSpareOptions {
            primary_serial: Some(10),
            spare_serial: Some(20),
            stdin_json: true,
        };

        let err = run_enroll_spare_with(options, &mut boundary)
            .err()
            .ok_or_else(|| anyhow::anyhow!("enroll-spare unexpectedly succeeded"))?;

        assert_eq!(err.to_string(), "pin verification failed");
        assert_eq!(boundary.enrollment_read_calls, 0);
        Ok(())
    }

    #[test]
    fn enroll_primary_rejects_empty_secret_before_setup() -> Result<()> {
        let (device, state) = FakeDevice::fresh_with_state(10);
        let session = SecretSession::start()?;
        let empty = protected_test_secret(b"", &session)?;
        let secrets = EnrollmentSecretSet::new(
            empty,
            protected_test_secret(b"pw", &session)?,
            protected_test_secret(b"token", &session)?,
        );

        let err = enroll_without_local_verify(
            &mut { device },
            domain::YubikeyRole::Primary,
            &secrets,
            &session,
        )
        .err()
        .ok_or_else(|| anyhow::anyhow!("enroll unexpectedly accepted empty bw-email"))?;

        assert_eq!(err.to_string(), "bw-email must not be empty");
        let state = state.borrow();
        assert!(!state.key_exists);
        assert!(state.objects.is_empty());
        Ok(())
    }

    #[test]
    fn enroll_spare_rejects_empty_secret_before_setup() -> Result<()> {
        let (device, state) = FakeDevice::fresh_with_state(20);
        let session = SecretSession::start()?;
        let empty = protected_test_secret(b"", &session)?;
        let secrets = EnrollmentSecretSet::new(
            protected_test_secret(b"user@example.com", &session)?,
            empty,
            protected_test_secret(b"token", &session)?,
        );

        let err = enroll_without_local_verify(
            &mut { device },
            domain::YubikeyRole::Spare,
            &secrets,
            &session,
        )
        .err()
        .ok_or_else(|| anyhow::anyhow!("enroll unexpectedly accepted empty bw-password"))?;

        assert_eq!(err.to_string(), "bw-password must not be empty");
        let state = state.borrow();
        assert!(!state.key_exists);
        assert!(state.objects.is_empty());
        Ok(())
    }

    fn protected_enrollment_secret_set<'session>(
        memory: &'session SecretSession,
    ) -> Result<EnrollmentSecretSet<'session>> {
        Ok(EnrollmentSecretSet::new(
            protected_test_secret(b"user@example.com", memory)?,
            protected_test_secret(b"password", memory)?,
            protected_test_secret(b"token", memory)?,
        ))
    }

    fn protected_test_secret<'session>(
        bytes: &'static [u8],
        memory: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>> {
        let input =
            ProtectedInputBuffer::read_from(Cursor::new(bytes), bytes.len(), "too large", memory)?;
        input.into_protected_secret(memory)
    }
}
