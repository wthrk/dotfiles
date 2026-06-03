//! verify-yubikey の device 解決順序を固定し、外部検証の責務境界を application に維持する。

use crate::Result;
use crate::secrets::{
    domain::{
        bw_login::{BwLoginEmail, BwOtp},
        commands::VerifyYubikeyCommand,
        piv::{SecretName, validate_piv_pin_len},
        storage::{SecretStorageReadIntent, SecretStorageVerificationPlan},
        verification::{CheckName, CheckStatus, VerifySummary},
    },
    ports,
    support::protection::{ProtectedSecret, bw_login},
};

/// 保存済み secret の存在と、要求された外部確認項目を検証する。
///
/// serial 未指定時の自動選択を device port 境界へ委譲し、local storage 検証を完了条件の
/// 先頭に固定する。外部確認結果は report 境界へ明示的に反映し、verify 結果の責任範囲を
/// 曖昧にしない。
#[expect(
    clippy::too_many_arguments,
    reason = "verify-yubikey は device/pin/storage/report/bws/otp-input/bw-login の port を順序適用する単一 use case"
)]
pub(crate) async fn run_verify_yubikey_with<D, P, S, R, B, O, L>(
    command: VerifyYubikeyCommand,
    device_serial: &mut D,
    pin_policy: &mut impl ports::DevicePinPolicyPort,
    process: &P,
    storage_port: &mut S,
    report: &R,
    bws_client: &B,
    otp_input: &O,
    bw_login_port: &L,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    P: ports::PinInputPort,
    S: ports::SecretStoragePort,
    R: ports::ReportPort,
    B: ports::BwsClientPort,
    O: ports::BwOtpInputPort,
    L: ports::BwLoginPort,
{
    let requested = command.requested_external_checks()?;
    let serial = device_serial.resolve_device_serial(command.serial)?;
    let pin = if pin_policy.device_requires_pin(serial)? {
        let pin = process.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };
    // local storage 検証（ローカル保管確認）: 3 secret（bw-email / bw-password / bws-access-token）すべての
    // 存在・復号可能性を inspect → intent → load → validate で検証する（yubikey-secret-storage-design.md L280）。
    // master password（`bw-password`）・`bw-email`・`bws-access-token` の lifetime を最小化するため、検証後は
    // いずれの secret も retain せず drop する。各外部確認が必要とする secret は、その分岐の直前で on-demand に
    // 読み直し、分岐を抜けると drop されるようにする。これにより master password は bw-login 確認の間だけ future
    // に存在し、BWS check の `.await` 中には保持されない。
    let local_verify = (|| {
        for storage in SecretStorageVerificationPlan::for_serial(serial).into_targets() {
            let inspection = storage_port.inspect_secret_storage_read(serial, &storage)?;
            let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
            let secret = storage_port
                .load_secret(serial, &intent, pin.as_ref())
                .map_err(|error| intent.decode_error(error))?;
            // local storage 検証範囲は縮小しない。3 secret すべての存在・復号可能性をここで検証する。
            // 検証後の secret は match で振り分けず drop し、retain しない。
            intent.validate_loaded_secret(&secret)?;
        }
        Ok(())
    })();
    if let Err(err) = local_verify {
        return report
            .write_verify_report(&VerifySummary::local_storage_failed(serial))
            .and(Err(err));
    }
    if requested.is_empty() {
        return report.write_verify_report(&VerifySummary::local_storage_verified(serial));
    }
    let mut summary = VerifySummary::local_storage_verified(serial);
    let mut first_error = None;
    for check in requested {
        match check {
            CheckName::Bws => {
                // bws-access-token をこの分岐の中で on-demand にロードし、分岐を抜けると drop されるようにする。
                // BWS 外部確認の `.await` の間だけ access token が future に存在する。
                let access_token = match load_yubikey_secret(
                    serial,
                    SecretName::BwsAccessToken,
                    storage_port,
                    pin.as_ref(),
                ) {
                    Ok(secret) => secret,
                    Err(error) => {
                        summary.mark_external_check(CheckName::Bws, CheckStatus::Failed);
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                        continue;
                    }
                };
                let access_token = &access_token;
                let mut result = Ok(());
                let project_id =
                    match bws_client
                        .list_bws_projects(access_token)
                        .await
                        .and_then(|projects| {
                            crate::secrets::domain::bws::BwsProjectName::DOTFILES_SECRET_RECOVERY
                                .resolve_id(projects)
                        }) {
                        Ok(project_id) => project_id,
                        Err(error) => {
                            result = Err(error);
                            summary.mark_external_check(CheckName::Bws, CheckStatus::Failed);
                            if first_error.is_none() {
                                first_error = result.err();
                            }
                            continue;
                        }
                    };
                let secret_candidates =
                    match bws_client.list_bws_secrets(access_token, &project_id).await {
                        Ok(secrets) => secrets,
                        Err(error) => {
                            result = Err(error);
                            summary.mark_external_check(CheckName::Bws, CheckStatus::Failed);
                            if first_error.is_none() {
                                first_error = result.err();
                            }
                            continue;
                        }
                    };
                for secret_name in check.required_bws_secrets().ok_or_else(|| {
                    anyhow::anyhow!("internal invariant violated: bws check has no secret plan")
                })? {
                    let secret_id =
                        match secret_name.resolve_id(secret_candidates.clone(), &project_id) {
                            Ok(secret_id) => secret_id,
                            Err(error) => {
                                result = Err(error);
                                break;
                            }
                        };
                    if let Err(error) = bws_client
                        .fetch_bws_secret_by_id(access_token, &secret_id)
                        .await
                        .map(|_| ())
                    {
                        result = Err(error);
                        break;
                    }
                }
                match result {
                    Ok(_) => summary.mark_external_check(CheckName::Bws, CheckStatus::Ok),
                    Err(error) => {
                        summary.mark_external_check(CheckName::Bws, CheckStatus::Failed);
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                    }
                }
            }
            CheckName::BwLogin => {
                // bw-login 外部確認は、`bw-email`（または `--email` override）/ `bw-password` と入力 OTP で
                // 実際に `bw login` / `bw unlock` の到達性を確認する（yubikey-secret-storage-design.md L286）。bw-login use case と
                // 同じ実行経路（`BwLoginPort`）を再利用し、master password は port の `BW_PASSWORD` env 境界で
                // だけ子プロセスへ渡る。session key は確認専用のため surface せず破棄する。
                //
                // email は override 指定時のみ override を `BwLoginEmail::parse` で検証し、未指定時は
                // `bw-email` をこの分岐の中で on-demand ロードして `bw_login::parse_email` で翻訳する。
                // `bw-password` もこの分岐の中で on-demand ロードして port へ渡す。両 secret は分岐を抜けると
                // drop され、BWS check の `.await` 中には保持されない。
                match run_bw_login_check(
                    command.email_override.as_deref(),
                    serial,
                    storage_port,
                    pin.as_ref(),
                    otp_input,
                    bw_login_port,
                )
                .await
                {
                    Ok(()) => summary.mark_external_check(CheckName::BwLogin, CheckStatus::Ok),
                    Err(error) => {
                        summary.mark_external_check(CheckName::BwLogin, CheckStatus::Failed);
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                    }
                }
            }
            CheckName::Setup
            | CheckName::BwEmail
            | CheckName::BwPassword
            | CheckName::BwsAccessToken
            | CheckName::LocalStorage => {
                unreachable!("requested_external_checks returned a non-external verification check")
            }
        }
    }
    report.write_verify_report(&summary)?;
    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(())
}

/// bw-login 外部確認を bw-login use case と同じ port 経路で実行する。
///
/// login email は `run_bw_login` の email 決定ロジックと同じ方針で決める。`--email` override が
/// 指定された場合は YubiKey の `bw-email` を読まず override を `BwLoginEmail::parse` で検証し、未指定の
/// 場合だけ `bw-email` をこの確認の中で on-demand ロードして protection 境界の内側で argv 安全な email へ翻訳する。
/// master password（`bw-password`）も on-demand ロードして port へ保護値として渡し、OTP を入力して `bw login`
/// / `bw unlock` の到達性を確認する。両 secret はこの関数のスコープでだけ存在し、戻ると drop される。
/// session key は確認専用のため受け取った値を surface せず破棄し、login / unlock の成否だけを返す。
async fn run_bw_login_check<S, O, L>(
    email_override: Option<&str>,
    serial: u32,
    storage_port: &mut S,
    pin: Option<&ProtectedSecret>,
    otp_input: &O,
    bw_login_port: &L,
) -> Result<()>
where
    S: ports::SecretStoragePort,
    O: ports::BwOtpInputPort,
    L: ports::BwLoginPort,
{
    // override 指定時は YubiKey の `bw-email` を読まない/使わない。未指定時のみ on-demand ロードする。
    let email: BwLoginEmail = match email_override {
        Some(value) => BwLoginEmail::parse(value)?,
        None => {
            let bw_email = load_yubikey_secret(serial, SecretName::BwEmail, storage_port, pin)?;
            bw_login::parse_email(&bw_email)?
        }
    };
    let bw_password = load_yubikey_secret(serial, SecretName::BwPassword, storage_port, pin)?;
    let otp = BwOtp::parse(&otp_input.read_bw_otp()?)?;
    bw_login_port
        .login_and_unlock(&email, &bw_password, &otp)
        .await
        .map(|_session| ())
}

/// YubiKey storage の read 経路（inspect → intent → load → validate）で指定 secret を on-demand 取得する。
///
/// この helper は application 層の順序制御（必要な分岐の直前で必要 secret だけを読み、戻ると drop する）に
/// 属するため `pub` にしない。pin はこの use case で取得済みの値を渡す（追加の touch を要しない read 操作）。
/// `run_bw_login.rs` の同名 helper と同方針だが use case-to-use case call を避けるため本 file に閉じる。
fn load_yubikey_secret<S>(
    serial: u32,
    name: SecretName,
    storage_port: &mut S,
    pin: Option<&ProtectedSecret>,
) -> Result<ProtectedSecret>
where
    S: ports::SecretStoragePort,
{
    let storage = name.storage_spec(serial);
    let inspection = storage_port.inspect_secret_storage_read(serial, &storage)?;
    let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
    let secret = storage_port
        .load_secret(serial, &intent, pin)
        .map_err(|error| intent.decode_error(error))?;
    intent.validate_loaded_secret(&secret)?;
    Ok(secret)
}

#[cfg(test)]
mod tests {
    use crate::secrets::{
        domain::{
            bws::{BwsLookupCandidate, BwsProjectId, BwsSecretId},
            commands::VerifyYubikeyCommand,
            manifest::SecretManifest,
            piv::SecretName,
            storage::SecretStorageReadInspection,
            verification::{CheckName, CheckStatus, ExternalCheck},
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_verify_yubikey_with;

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn read_inspection() -> SecretStorageReadInspection {
        SecretStorageReadInspection {
            manifest_bytes: Some(SecretManifest::expected().encode().expect("manifest")),
            encoded: Some(vec![1]),
        }
    }

    /// secret 名から決め打ちの test 値を返す。on-demand ロードと local 検証で同じ値を返す。
    fn secret_value(name: SecretName) -> ProtectedSecret {
        match name {
            SecretName::BwEmail => material(b"email"),
            SecretName::BwPassword => material(b"password"),
            SecretName::BwsAccessToken => material(b"access-token"),
        }
    }

    /// local storage 検証（3 secret すべての inspect → load）を順序付きで 1 巡分期待する。
    ///
    /// local 検証範囲は縮小しないため、bw-email / bw-password / bws-access-token の 3 secret を必ず
    /// この順で inspect/load する。各 secret は検証後に drop され retain されない。
    fn expect_local_storage_ok(
        storage: &mut ports::MockSecretStoragePort,
        sequence: &mut mockall::Sequence,
        serial: u32,
    ) {
        for name in [
            SecretName::BwEmail,
            SecretName::BwPassword,
            SecretName::BwsAccessToken,
        ] {
            expect_secret_load(storage, sequence, serial, name);
        }
    }

    /// 指定 secret の inspect → load を 1 回ずつ順序付きで期待する（on-demand ロード 1 回分）。
    fn expect_secret_load(
        storage: &mut ports::MockSecretStoragePort,
        sequence: &mut mockall::Sequence,
        serial: u32,
        name: SecretName,
    ) {
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .in_sequence(sequence)
            .withf(move |actual_serial, storage| *actual_serial == serial && storage.name == name)
            .returning(|_, _| Ok(read_inspection()));
        storage
            .expect_load_secret()
            .times(1)
            .in_sequence(sequence)
            .withf(move |actual_serial, intent, _| {
                *actual_serial == serial && intent.storage.name == name
            })
            .returning(move |_, intent, _| Ok(secret_value(intent.storage.name)));
    }

    #[tokio::test]
    async fn verify_rejects_conflicting_external_check_flags_before_ports() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        device_serial.expect_resolve_device_serial().times(0);
        let process = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        storage.expect_inspect_secret_storage_read().times(0);
        let report = ports::MockReportPort::new();
        let bws = ports::MockBwsClientPort::new();
        let otp_input = ports::MockBwOtpInputPort::new();
        let bw_login = ports::MockBwLoginPort::new();

        let result = run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: Some(2001),
                checks: vec![ExternalCheck::Bws],
                all: true,
                email_override: None,
            },
            &mut device_serial,
            &mut pin_policy,
            &process,
            &mut storage,
            &report,
            &bws,
            &otp_input,
            &bw_login,
        )
        .await;

        assert!(result.is_err(), "--all and --check cannot be used together");
    }

    #[tokio::test]
    async fn verify_bws_check_fetches_required_secrets_and_reports_ok() -> crate::Result<()> {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        let mut sequence = mockall::Sequence::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|requested| Ok(requested.expect("serial")));
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(false));

        let process = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence, 2001);
        // BWS 分岐は bws-access-token を on-demand ロードする（bw-password はロードしない）。
        expect_secret_load(
            &mut storage,
            &mut sequence,
            2001,
            SecretName::BwsAccessToken,
        );

        let mut bws = ports::MockBwsClientPort::new();
        let otp_input = ports::MockBwOtpInputPort::new();
        let bw_login = ports::MockBwLoginPort::new();
        bws.expect_list_bws_projects()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| {
                Ok(vec![BwsLookupCandidate {
                    id: BwsProjectId::new("project-1"),
                    name: "dotfiles-secret-recovery".to_owned(),
                }])
            });
        bws.expect_list_bws_secrets()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|_, project_id| project_id.as_str() == "project-1")
            .returning(|_, _| {
                Ok(vec![
                    BwsLookupCandidate {
                        id: BwsSecretId::new("gpg-id"),
                        name: "gpg-secret-key-backup".to_owned(),
                    },
                    BwsLookupCandidate {
                        id: BwsSecretId::new("pass-id"),
                        name: "password-store-remote".to_owned(),
                    },
                ])
            });
        bws.expect_fetch_bws_secret_by_id()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|_, secret_id| secret_id.as_str() == "gpg-id")
            .returning(|_, _| Ok(material(b"gpg")));
        bws.expect_fetch_bws_secret_by_id()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|_, secret_id| secret_id.as_str() == "pass-id")
            .returning(|_, _| Ok(material(b"pass")));

        let mut report = ports::MockReportPort::new();
        report
            .expect_write_verify_report()
            .times(1)
            .withf(|summary| {
                summary.serial == 2001
                    && summary.checks.get(&CheckName::LocalStorage) == Some(&CheckStatus::Ok)
                    && summary.checks.get(&CheckName::Bws) == Some(&CheckStatus::Ok)
            })
            .returning(|_| Ok(()));

        run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: Some(2001),
                checks: vec![ExternalCheck::Bws],
                all: false,
                email_override: None,
            },
            &mut device_serial,
            &mut pin_policy,
            &process,
            &mut storage,
            &report,
            &bws,
            &otp_input,
            &bw_login,
        )
        .await
    }

    #[tokio::test]
    async fn verify_bws_check_reports_project_lookup_failure() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        let mut sequence = mockall::Sequence::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(false));
        let process = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence, 2001);
        // BWS 分岐は bws-access-token を on-demand ロードする（bw-password はロードしない）。
        expect_secret_load(
            &mut storage,
            &mut sequence,
            2001,
            SecretName::BwsAccessToken,
        );
        let mut bws = ports::MockBwsClientPort::new();
        let otp_input = ports::MockBwOtpInputPort::new();
        let bw_login = ports::MockBwLoginPort::new();
        bws.expect_list_bws_projects()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(Vec::new()));
        bws.expect_list_bws_secrets().times(0);
        bws.expect_fetch_bws_secret_by_id().times(0);
        let mut report = ports::MockReportPort::new();
        report
            .expect_write_verify_report()
            .times(1)
            .withf(|summary| summary.checks.get(&CheckName::Bws) == Some(&CheckStatus::Failed))
            .returning(|_| Ok(()));

        let result = run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: Some(2001),
                checks: vec![ExternalCheck::Bws],
                all: false,
                email_override: None,
            },
            &mut device_serial,
            &mut pin_policy,
            &process,
            &mut storage,
            &report,
            &bws,
            &otp_input,
            &bw_login,
        )
        .await;

        assert!(result.is_err(), "missing BWS project must fail");
    }

    #[tokio::test]
    async fn verify_bws_check_reports_secret_lookup_failure() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        let mut sequence = mockall::Sequence::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(false));
        let process = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence, 2001);
        // BWS 分岐は bws-access-token を on-demand ロードする（bw-password はロードしない）。
        expect_secret_load(
            &mut storage,
            &mut sequence,
            2001,
            SecretName::BwsAccessToken,
        );
        let mut bws = ports::MockBwsClientPort::new();
        let otp_input = ports::MockBwOtpInputPort::new();
        let bw_login = ports::MockBwLoginPort::new();
        bws.expect_list_bws_projects()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| {
                Ok(vec![BwsLookupCandidate {
                    id: BwsProjectId::new("project-1"),
                    name: "dotfiles-secret-recovery".to_owned(),
                }])
            });
        bws.expect_list_bws_secrets()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(Vec::new()));
        bws.expect_fetch_bws_secret_by_id().times(0);
        let mut report = ports::MockReportPort::new();
        report
            .expect_write_verify_report()
            .times(1)
            .withf(|summary| summary.checks.get(&CheckName::Bws) == Some(&CheckStatus::Failed))
            .returning(|_| Ok(()));

        let result = run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: Some(2001),
                checks: vec![ExternalCheck::Bws],
                all: false,
                email_override: None,
            },
            &mut device_serial,
            &mut pin_policy,
            &process,
            &mut storage,
            &report,
            &bws,
            &otp_input,
            &bw_login,
        )
        .await;

        assert!(result.is_err(), "missing BWS secret must fail");
    }

    #[tokio::test]
    async fn verify_bws_check_reports_fetch_failure() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        let mut sequence = mockall::Sequence::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(false));
        let process = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence, 2001);
        // BWS 分岐は bws-access-token を on-demand ロードする（bw-password はロードしない）。
        expect_secret_load(
            &mut storage,
            &mut sequence,
            2001,
            SecretName::BwsAccessToken,
        );
        let mut bws = ports::MockBwsClientPort::new();
        let otp_input = ports::MockBwOtpInputPort::new();
        let bw_login = ports::MockBwLoginPort::new();
        bws.expect_list_bws_projects()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| {
                Ok(vec![BwsLookupCandidate {
                    id: BwsProjectId::new("project-1"),
                    name: "dotfiles-secret-recovery".to_owned(),
                }])
            });
        bws.expect_list_bws_secrets()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| {
                Ok(vec![
                    BwsLookupCandidate {
                        id: BwsSecretId::new("gpg-id"),
                        name: "gpg-secret-key-backup".to_owned(),
                    },
                    BwsLookupCandidate {
                        id: BwsSecretId::new("pass-id"),
                        name: "password-store-remote".to_owned(),
                    },
                ])
            });
        bws.expect_fetch_bws_secret_by_id()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Err(anyhow::anyhow!("fetch failed")));
        let mut report = ports::MockReportPort::new();
        report
            .expect_write_verify_report()
            .times(1)
            .withf(|summary| summary.checks.get(&CheckName::Bws) == Some(&CheckStatus::Failed))
            .returning(|_| Ok(()));

        let result = run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: Some(2001),
                checks: vec![ExternalCheck::Bws],
                all: false,
                email_override: None,
            },
            &mut device_serial,
            &mut pin_policy,
            &process,
            &mut storage,
            &report,
            &bws,
            &otp_input,
            &bw_login,
        )
        .await;

        assert!(result.is_err(), "BWS fetch failure must fail");
    }

    #[tokio::test]
    async fn verify_bw_login_check_logs_in_and_reports_ok() -> crate::Result<()> {
        use crate::secrets::domain::bw_login::{BwLoginEmail, BwOtp, BwSessionKey};

        let mut device_serial = ports::MockDeviceSerialPort::new();
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        let mut sequence = mockall::Sequence::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|requested| Ok(requested.expect("serial")));
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(false));

        let process = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence, 2001);
        // bw-login 分岐（override 未指定）は bw-email → bw-password を on-demand ロードする。
        // bws-access-token はロードしない。
        expect_secret_load(&mut storage, &mut sequence, 2001, SecretName::BwEmail);
        expect_secret_load(&mut storage, &mut sequence, 2001, SecretName::BwPassword);

        // bw-login 確認では BWS port は呼ばれない。
        let bws = ports::MockBwsClientPort::new();

        let mut otp_input = ports::MockBwOtpInputPort::new();
        otp_input
            .expect_read_bw_otp()
            .times(1)
            .returning(|| Ok("cccccbtdvuotp".to_owned()));

        // local storage で load した bw-email（"email"）/ bw-password（"password"）が port へ渡る。
        let mut bw_login = ports::MockBwLoginPort::new();
        bw_login
            .expect_login_and_unlock()
            .times(1)
            .withf(
                |email: &BwLoginEmail, password: &ProtectedSecret, otp: &BwOtp| {
                    email.as_str() == "email"
                        && otp.as_str() == "cccccbtdvuotp"
                        && *password == material(b"password")
                },
            )
            .returning(|_, _, _| Ok(BwSessionKey::parse("SESSIONKEY==").expect("session")));

        let mut report = ports::MockReportPort::new();
        report
            .expect_write_verify_report()
            .times(1)
            .withf(|summary| {
                summary.checks.get(&CheckName::LocalStorage) == Some(&CheckStatus::Ok)
                    && summary.checks.get(&CheckName::BwLogin) == Some(&CheckStatus::Ok)
                    && summary.checks.get(&CheckName::Bws) == Some(&CheckStatus::Skipped)
            })
            .returning(|_| Ok(()));

        run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: Some(2001),
                checks: vec![ExternalCheck::BwLogin],
                all: false,
                email_override: None,
            },
            &mut device_serial,
            &mut pin_policy,
            &process,
            &mut storage,
            &report,
            &bws,
            &otp_input,
            &bw_login,
        )
        .await
    }

    /// `--check bw-login --email <override>` は override email で bw-login 確認を行い、bw-login の email 決定に
    /// YubiKey の `bw-email`（local storage では "email"）を使わないことを検証する（yubikey-secret-storage-design.md L286）。local storage 検証
    /// 範囲は縮小しないため、bw-email を含む 3 secret は引き続き inspect/load/validate される。
    #[tokio::test]
    async fn verify_bw_login_check_uses_email_override() -> crate::Result<()> {
        use crate::secrets::domain::bw_login::{BwLoginEmail, BwOtp, BwSessionKey};

        let mut device_serial = ports::MockDeviceSerialPort::new();
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        let mut sequence = mockall::Sequence::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|requested| Ok(requested.expect("serial")));
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(false));

        let process = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        // local storage 検証範囲は縮小しない: bw-email を含む 3 secret を引き続き検証する。
        expect_local_storage_ok(&mut storage, &mut sequence, 2001);
        // override 指定時、bw-login 分岐は bw-email を on-demand ロードせず bw-password だけをロードする。
        // bw-email の inspect/load は local 検証の 1 回のみで、追加の on-demand 期待を置かないことで、
        // override 経路が YubiKey の bw-email を読まないこと（読めば未マッチで panic）を担保する。
        expect_secret_load(&mut storage, &mut sequence, 2001, SecretName::BwPassword);

        let bws = ports::MockBwsClientPort::new();

        let mut otp_input = ports::MockBwOtpInputPort::new();
        otp_input
            .expect_read_bw_otp()
            .times(1)
            .returning(|| Ok("cccccbtdvuotp".to_owned()));

        // override email が port へ渡り、YubiKey の `bw-email`（"email"）は bw-login 決定に使われない。
        let mut bw_login = ports::MockBwLoginPort::new();
        bw_login
            .expect_login_and_unlock()
            .times(1)
            .withf(
                |email: &BwLoginEmail, password: &ProtectedSecret, otp: &BwOtp| {
                    email.as_str() == "override@example.com"
                        && otp.as_str() == "cccccbtdvuotp"
                        && *password == material(b"password")
                },
            )
            .returning(|_, _, _| Ok(BwSessionKey::parse("SESSIONKEY==").expect("session")));

        let mut report = ports::MockReportPort::new();
        report
            .expect_write_verify_report()
            .times(1)
            .withf(|summary| {
                summary.checks.get(&CheckName::LocalStorage) == Some(&CheckStatus::Ok)
                    && summary.checks.get(&CheckName::BwLogin) == Some(&CheckStatus::Ok)
            })
            .returning(|_| Ok(()));

        run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: Some(2001),
                checks: vec![ExternalCheck::BwLogin],
                all: false,
                email_override: Some("  override@example.com  ".to_owned()),
            },
            &mut device_serial,
            &mut pin_policy,
            &process,
            &mut storage,
            &report,
            &bws,
            &otp_input,
            &bw_login,
        )
        .await
    }

    #[tokio::test]
    async fn verify_bw_login_check_reports_failure_when_login_fails() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        let mut sequence = mockall::Sequence::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(false));
        let process = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence, 2001);
        // bw-login 分岐（override 未指定）は bw-email → bw-password を on-demand ロードする。
        expect_secret_load(&mut storage, &mut sequence, 2001, SecretName::BwEmail);
        expect_secret_load(&mut storage, &mut sequence, 2001, SecretName::BwPassword);
        let bws = ports::MockBwsClientPort::new();
        let mut otp_input = ports::MockBwOtpInputPort::new();
        otp_input
            .expect_read_bw_otp()
            .times(1)
            .returning(|| Ok("cccccbtdvuotp".to_owned()));
        let mut bw_login = ports::MockBwLoginPort::new();
        bw_login
            .expect_login_and_unlock()
            .times(1)
            .returning(|_, _, _| anyhow::bail!("bw login failed"));
        let mut report = ports::MockReportPort::new();
        report
            .expect_write_verify_report()
            .times(1)
            .withf(|summary| summary.checks.get(&CheckName::BwLogin) == Some(&CheckStatus::Failed))
            .returning(|_| Ok(()));

        let result = run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: Some(2001),
                checks: vec![ExternalCheck::BwLogin],
                all: false,
                email_override: None,
            },
            &mut device_serial,
            &mut pin_policy,
            &process,
            &mut storage,
            &report,
            &bws,
            &otp_input,
            &bw_login,
        )
        .await;

        assert!(result.is_err(), "bw-login failure must fail verify");
    }

    /// `--all` は Bws → BwLogin の両 check を走らせ、各分岐がその直前で必要 secret を on-demand ロードする
    /// ことを検証する。Bws 分岐は bws-access-token を、BwLogin 分岐（override 未指定）は bw-email → bw-password
    /// を読む。各 secret は local 検証で一度、各分岐の on-demand で再度ロードされる。
    #[tokio::test]
    async fn verify_all_runs_both_checks_loading_secrets_per_branch() -> crate::Result<()> {
        use crate::secrets::domain::bw_login::{BwLoginEmail, BwOtp, BwSessionKey};

        let mut device_serial = ports::MockDeviceSerialPort::new();
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        let mut sequence = mockall::Sequence::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|requested| Ok(requested.expect("serial")));
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(false));

        let process = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        // local 検証（3 secret）→ Bws 分岐 access-token を順序付きで期待する。BwLogin 分岐の
        // bw-email → bw-password は BWS port 呼び出しの後に続けて期待する（後段で sequence に追加）。
        expect_local_storage_ok(&mut storage, &mut sequence, 2001);
        expect_secret_load(
            &mut storage,
            &mut sequence,
            2001,
            SecretName::BwsAccessToken,
        );

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| {
                Ok(vec![BwsLookupCandidate {
                    id: BwsProjectId::new("project-1"),
                    name: "dotfiles-secret-recovery".to_owned(),
                }])
            });
        bws.expect_list_bws_secrets()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| {
                Ok(vec![
                    BwsLookupCandidate {
                        id: BwsSecretId::new("gpg-id"),
                        name: "gpg-secret-key-backup".to_owned(),
                    },
                    BwsLookupCandidate {
                        id: BwsSecretId::new("pass-id"),
                        name: "password-store-remote".to_owned(),
                    },
                ])
            });
        bws.expect_fetch_bws_secret_by_id()
            .times(2)
            .returning(|_, _| Ok(material(b"secret")));

        // BWS 分岐の後、BwLogin 分岐（override 未指定）が bw-email → bw-password を on-demand ロードする。
        expect_secret_load(&mut storage, &mut sequence, 2001, SecretName::BwEmail);
        expect_secret_load(&mut storage, &mut sequence, 2001, SecretName::BwPassword);

        let mut otp_input = ports::MockBwOtpInputPort::new();
        otp_input
            .expect_read_bw_otp()
            .times(1)
            .returning(|| Ok("cccccbtdvuotp".to_owned()));

        let mut bw_login = ports::MockBwLoginPort::new();
        bw_login
            .expect_login_and_unlock()
            .times(1)
            .withf(
                |email: &BwLoginEmail, password: &ProtectedSecret, otp: &BwOtp| {
                    email.as_str() == "email"
                        && otp.as_str() == "cccccbtdvuotp"
                        && *password == material(b"password")
                },
            )
            .returning(|_, _, _| Ok(BwSessionKey::parse("SESSIONKEY==").expect("session")));

        let mut report = ports::MockReportPort::new();
        report
            .expect_write_verify_report()
            .times(1)
            .withf(|summary| {
                summary.checks.get(&CheckName::LocalStorage) == Some(&CheckStatus::Ok)
                    && summary.checks.get(&CheckName::Bws) == Some(&CheckStatus::Ok)
                    && summary.checks.get(&CheckName::BwLogin) == Some(&CheckStatus::Ok)
            })
            .returning(|_| Ok(()));

        run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: Some(2001),
                checks: Vec::new(),
                all: true,
                email_override: None,
            },
            &mut device_serial,
            &mut pin_policy,
            &process,
            &mut storage,
            &report,
            &bws,
            &otp_input,
            &bw_login,
        )
        .await
    }
}
