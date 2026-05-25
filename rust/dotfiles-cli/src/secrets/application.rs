//! `dotfiles secrets` の application 層。
//!
//! この層は command ごとの use case と外部境界の順序を所有する。secret を読む前に
//! device と非対話条件を確定し、平文 secret は `SecretSession` に紐づく保護済み値として
//! domain の保存操作へ渡す。

mod storage_service;
pub(super) mod summary;

use std::collections::BTreeSet;

use super::{
    domain::SecretName,
    ports::{self, SecretDevice, SecretsBoundary},
    support::protection::{ProtectedInputBuffer, ProtectedSecret, SecretSession},
    EnrollSpareOptions, EnrollmentSecretSet, SecretsCommand, SecretsOptions, VerifyCheck,
    VerifyYubikeyOptions, YubikeyCommand, YubikeyOptions,
};
use crate::Result;
use anyhow::bail;

/// stdin JSON の最大許容 byte 数。
const MAX_BOOTSTRAP_JSON_LEN: usize = 64 * 1024;
/// stdin から読む single secret の最大 byte 数。
const MAX_SINGLE_STDIN_SECRET_LEN: usize = 16 * 1024;

const NONINTERACTIVE_SERIAL_ERROR: &str = "pass --serial in non-interactive use";
const NONINTERACTIVE_PRIMARY_SERIAL_ERROR: &str = "pass --primary-serial in non-interactive use";
const NONINTERACTIVE_SPARE_SERIAL_ERROR: &str = "pass --spare-serial in non-interactive use";

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
    boundary.require_serial(options.serial, NONINTERACTIVE_SERIAL_ERROR)?;
    let mut device = boundary.open_device(options.serial)?;
    storage_service::setup(&mut device)
}

/// 単一 secret を読み込み、指定された storage object へ保存する。
///
/// 既存 object の上書き可否は secret 入力より前に確定する。
fn run_put_with<B: SecretsBoundary>(options: super::PutOptions, boundary: &mut B) -> Result<()> {
    boundary.require_serial(options.serial, NONINTERACTIVE_SERIAL_ERROR)?;
    require_single_stdin_secret_source(options.stdin, boundary)?;
    let session = SecretSession::start()?;
    let mut device = boundary.open_device(options.serial)?;
    session.run_operation(|| {
        storage_service::check_put_preconditions(&mut device, options.name, options.force)
    })?;
    let secret = read_protected_secret_for_put(boundary, options.name, options.stdin, &session)?;
    secret.with_secret(|secret| {
        session.run_operation(|| {
            storage_service::put(&mut device, options.name, secret, options.force, &session)
        })
    })
}

/// 指定された secret を device から復号し、stdout へ書き込む。
///
/// stdout が pipe/redirect でない場合は、PIN verification と touch の前に停止する。
fn run_get_with<B: SecretsBoundary>(options: super::GetOptions, boundary: &mut B) -> Result<()> {
    boundary.require_serial(options.serial, NONINTERACTIVE_SERIAL_ERROR)?;
    boundary.require_stdout_pipe()?;
    let session = SecretSession::start()?;
    let mut device = boundary.open_device(options.serial)?;
    verify_pin_for_secret_reads(boundary, &mut device, &session)?;
    let output_bytes = session.run_operation(|| {
        storage_service::get_protected(&mut device, options.name, &session)
    })?;
    output_bytes.with_secret(|bytes| boundary.write_secret_to_stdout(bytes))?;
    Ok(())
}

/// primary 用 enrollment secrets を読み込み、device へ登録して local verify まで実行する。
///
/// storage 衝突確認が終わるまでは enrollment secrets を読み始めない。
fn run_enroll_primary_with<B: SecretsBoundary>(
    options: super::EnrollPrimaryOptions,
    boundary: &mut B,
) -> Result<()> {
    boundary.require_serial(options.serial, NONINTERACTIVE_SERIAL_ERROR)?;
    boundary.require_stdin_json_pipe(options.stdin_json)?;
    boundary.require_option(options.stdin_json, "--stdin-json")?;
    let session = SecretSession::start()?;
    let mut device = boundary.open_device(options.serial)?;
    session.run_operation(|| storage_service::check_setup_preconditions(&mut device))?;
    let summary = {
        if options.stdin_json {
            verify_pin_for_secret_reads(boundary, &mut device, &session)?;
        }
        let secrets = read_enrollment_secret_set_from_user(boundary, options.stdin_json, &session)?;
        session.check_interrupted()?;
        if !options.stdin_json {
            verify_pin_for_secret_reads(boundary, &mut device, &session)?;
        }
        let mut summary = session.run_operation(|| {
            enroll_without_local_verify(
                &mut device,
                summary::YubikeyRole::Primary,
                &secrets,
                &session,
            )
        })?;
        verify_local_storage_protected(&mut device, &session)?;
        summary
            .checks
            .insert(summary::CheckName::LocalStorage, summary::CheckStatus::Ok);
        summary
    };
    boundary.write_report(&summary)?;
    Ok(())
}

/// `put` 用の単一 secret を prompt または stdin から読み込む。
///
/// bytes を受け取り、session 所属の保護済み値へ移す。
fn read_protected_secret_for_put<'session, B: SecretsBoundary>(
    boundary: &B,
    name: SecretName,
    stdin: bool,
    memory: &'session SecretSession,
) -> Result<ProtectedSecret<'session>> {
    let bytes = if stdin {
        boundary.read_stdin_bytes(MAX_SINGLE_STDIN_SECRET_LEN)?
    } else {
        boundary.read_hidden_bytes(&format!("{}: ", name), MAX_SINGLE_STDIN_SECRET_LEN)?
    };
    protect_bytes(bytes, memory)
}

/// 登録用の 3 field を prompt または stdin JSON から読み込む。
///
/// field ごとの保護境界を同じ session にそろえてから登録用 model にする。
fn read_enrollment_secret_set_from_user<'session, B: SecretsBoundary>(
    boundary: &B,
    stdin_json: bool,
    memory: &'session SecretSession,
) -> Result<EnrollmentSecretSet<'session>> {
    if stdin_json {
        let enrollment = boundary
            .read_enrollment_json_bytes(MAX_BOOTSTRAP_JSON_LEN, MAX_SINGLE_STDIN_SECRET_LEN)?;
        let bw_email = protect_bytes(enrollment.bw_email, memory)?;
        let bw_password = protect_bytes(enrollment.bw_password, memory)?;
        let bws_access_token = protect_bytes(enrollment.bws_access_token, memory)?;
        return Ok(EnrollmentSecretSet::new(
            bw_email,
            bw_password,
            bws_access_token,
        ));
    }

    let bw_email = protect_bytes(
        boundary.read_visible_line_bytes("bw-email: ", MAX_SINGLE_STDIN_SECRET_LEN)?,
        memory,
    )?;
    let bw_password =
        read_protected_secret_for_put(boundary, SecretName::BwPassword, false, memory)?;
    let bws_access_token =
        read_protected_secret_for_put(boundary, SecretName::BwsAccessToken, false, memory)?;

    Ok(EnrollmentSecretSet::new(
        bw_email,
        bw_password,
        bws_access_token,
    ))
}

/// zeroize 保護済み bytes を session 所属の `ProtectedSecret` へ移す。
fn protect_bytes<'session>(
    bytes: zeroize::Zeroizing<Vec<u8>>,
    session: &'session SecretSession,
) -> Result<ProtectedSecret<'session>> {
    let len = bytes.len();
    let cursor = std::io::Cursor::new(bytes.as_ref() as &[u8]);
    let buf = ProtectedInputBuffer::read_from(cursor, len, "secret is too large", session)?;
    buf.into_protected_secret(session)
}

/// spare 用 enrollment secrets を取得し、別 device へ登録して local verify まで実行する。
///
/// primary から復号する経路では、復号前に spare 候補と serial 制約を確定する。
fn run_enroll_spare_with<B: SecretsBoundary>(
    options: EnrollSpareOptions,
    boundary: &mut B,
) -> Result<()> {
    if !options.stdin_json {
        boundary.require_serial(options.primary_serial, NONINTERACTIVE_PRIMARY_SERIAL_ERROR)?;
    }
    boundary.require_serial(options.spare_serial, NONINTERACTIVE_SPARE_SERIAL_ERROR)?;
    boundary.require_stdin_json_pipe(options.stdin_json)?;
    let session = SecretSession::start()?;
    let prepared_spare = if options.stdin_json || options.spare_serial.is_some() {
        let mut spare = boundary.open_spare_device(options.spare_serial, options.primary_serial)?;
        session.run_operation(|| storage_service::check_setup_preconditions(&mut spare))?;
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
            read_enrollment_secret_set_from_user(boundary, true, &session)?,
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
            let mut spare = boundary.open_spare_device(options.spare_serial, primary_serial)?;
            session
                .run_operation(|| storage_service::check_setup_preconditions(&mut spare))?;
            spare
        }
    };

    session.check_interrupted()?;
    if !options.stdin_json {
        verify_pin_for_secret_reads(boundary, &mut spare, &session)?;
    }
    let mut summary = session.run_operation(|| {
        enroll_without_local_verify(
            &mut spare,
            summary::YubikeyRole::Spare,
            &bootstrap,
            &session,
        )
    })?;
    verify_local_storage_protected(&mut spare, &session)?;
    summary
        .checks
        .insert(summary::CheckName::LocalStorage, summary::CheckStatus::Ok);
    drop(bootstrap);
    boundary.write_report(&summary)?;
    Ok(())
}

/// primary device から 登録用の 3 field を復号する。
///
/// 各 field は次の device 操作前に session 所属の保護済み値へ移す。
fn read_protected_bootstrap_from_device<'session, D: ports::SecretDevice>(
    primary: &mut D,
    session: &'session SecretSession,
) -> Result<EnrollmentSecretSet<'session>> {
    let bw_email = session.run_operation(|| {
        storage_service::get_protected(primary, SecretName::BwEmail, session)
    })?;
    let bw_password = session.run_operation(|| {
        storage_service::get_protected(primary, SecretName::BwPassword, session)
    })?;
    let bws_access_token = session.run_operation(|| {
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
fn enroll_without_local_verify<D: ports::SecretDevice>(
    device: &mut D,
    role: summary::YubikeyRole,
    secrets: &EnrollmentSecretSet<'_>,
    session: &SecretSession,
) -> Result<summary::EnrollSummary> {
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
        let token = read_protected_secret_for_put(
            boundary,
            SecretName::BwsAccessToken,
            options.stdin,
            &session,
        )?;
        let rotation = rotate_bws_token_on_device(&mut device, &token, &session)?;
        drop(token);
        boundary.write_report(&rotation.summary)?;
        return rotation.result;
    }

    boundary.require_serial(None, NONINTERACTIVE_SERIAL_ERROR)?;
    boundary.require_option(options.stdin, "--stdin")?;
    let mut device = boundary.open_device(None)?;
    prepare_bws_token_rotation_device(boundary, &mut device, &session)?;
    let token = read_protected_secret_for_put(
        boundary,
        SecretName::BwsAccessToken,
        options.stdin,
        &session,
    )?;
    let mut updated_serials = BTreeSet::from([device.serial()]);
    let first_rotation = rotate_bws_token_on_device(&mut device, &token, &session)?;
    let mut summaries = vec![first_rotation.summary];
    if let Err(err) = first_rotation.result {
        drop(token);
        write_partial_rotate_bws_token_summary(boundary, &summaries)?;
        return Err(err);
    }
    drop(device);

    let remaining_result = (|| -> Result<()> {
        while session.run_operation(|| boundary.prompt_continue_rotation())? {
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
        write_partial_rotate_bws_token_summary(boundary, &summaries)?;
        return Err(err);
    }

    drop(token);
    boundary.write_report(&summaries)?;
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
fn rotate_bws_token_on_device<D: ports::SecretDevice>(
    device: &mut D,
    token: &ProtectedSecret<'_>,
    session: &SecretSession,
) -> Result<RotateBwsTokenResult> {
    session.run_operation(|| {
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
    summary: summary::VerifySummary,
    result: Result<()>,
}

#[derive(serde::Serialize)]
struct PartialRotateBwsTokenSummary<'a> {
    updated: &'a [summary::VerifySummary],
}

/// rotation 済み device の summary を部分成功 JSON として stdout へ出力する。
///
/// 途中失敗時に、利用者が再実行対象を判別できる情報を残す。
fn write_partial_rotate_bws_token_summary<B: SecretsBoundary>(
    boundary: &B,
    summaries: &[summary::VerifySummary],
) -> Result<()> {
    if summaries.is_empty() {
        return Ok(());
    }

    let partial = PartialRotateBwsTokenSummary { updated: summaries };
    boundary.write_report(&partial)
}

fn failed_local_storage_summary(serial: u32) -> summary::VerifySummary {
    summary::VerifySummary {
        serial,
        checks: [
            (
                summary::CheckName::LocalStorage,
                summary::CheckStatus::Failed,
            ),
            (summary::CheckName::Bws, summary::CheckStatus::Skipped),
            (summary::CheckName::BwLogin, summary::CheckStatus::Skipped),
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
    boundary.require_serial(options.serial, NONINTERACTIVE_SERIAL_ERROR)?;
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
            summary.checks.insert(*check, summary::CheckStatus::Failed);
        }
        boundary.write_report(&summary)?;
        let requested_names = requested
            .iter()
            .map(|check| match check {
                summary::CheckName::Bws => "bws",
                summary::CheckName::BwLogin => "bw-login",
                _ => unreachable!("requested_external_checks returns only external checks"),
            })
            .collect::<Vec<_>>()
            .join(", ");
        bail!("external checks are not implemented yet: {requested_names}");
    }

    boundary.write_report(&summary)?;
    Ok(())
}

fn requested_external_checks(options: &VerifyYubikeyOptions) -> Vec<summary::CheckName> {
    if options.all {
        return vec![summary::CheckName::Bws, summary::CheckName::BwLogin];
    }
    options
        .check
        .iter()
        .map(|check| match check {
            VerifyCheck::Bws => summary::CheckName::Bws,
            VerifyCheck::BwLogin => summary::CheckName::BwLogin,
        })
        .collect()
}

/// device 上の local storage secrets を復号し、空でないことを確認する。
///
/// 復号結果は空判定前に session の保護境界へ移す。
fn verify_local_storage_protected<D: ports::SecretDevice>(
    device: &mut D,
    session: &SecretSession,
) -> Result<summary::VerifySummary> {
    for name in SecretName::iter() {
        let secret = session
            .run_operation(|| storage_service::get_protected(device, name, session))?;
        secret.with_secret(|secret| {
            if secret.is_empty() {
                bail!("{} stored on this YubiKey is empty", name);
            }
            Ok(())
        })?;
    }

    Ok(summary::VerifySummary {
        serial: device.serial(),
        checks: [
            (summary::CheckName::LocalStorage, summary::CheckStatus::Ok),
            (summary::CheckName::Bws, summary::CheckStatus::Skipped),
            (summary::CheckName::BwLogin, summary::CheckStatus::Skipped),
        ]
        .into_iter()
        .collect(),
    })
}

/// rotation の書き込み前条件として local verify と management auth を確認する。
///
/// 確認は token 入力前に現在の保護境界内で実行する。
fn check_rotate_preconditions_protected<D: ports::SecretDevice>(
    device: &mut D,
    session: &SecretSession,
) -> Result<()> {
    verify_local_storage_protected(device, session)?;
    session.run_operation(|| device.check_management_auth_preconditions())
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

    let pin_bytes = boundary.read_yubikey_pin_bytes()?;
    let pin = protect_bytes(pin_bytes, session)?;
    pin.with_secret(|pin| session.run_operation(|| device.verify_pin(pin)))
}

/// 単一 secret を stdin から読む command の入力契約を確認する。
///
/// 非対話では `--stdin` を必須にし、TTY stdin では hidden prompt と混同しないよう拒否する。
fn require_single_stdin_secret_source<B: SecretsBoundary>(stdin: bool, boundary: &B) -> Result<()> {
    if stdin {
        return boundary.require_stdin_pipe();
    }
    boundary.require_option(false, "--stdin")
}
