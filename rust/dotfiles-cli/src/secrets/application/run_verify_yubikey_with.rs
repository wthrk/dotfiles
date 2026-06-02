//! verify-yubikey の device 解決順序を固定し、外部検証の責務境界を application に維持する。

use crate::Result;
use crate::secrets::{
    domain::{
        bw_login::{BwLoginEmail, BwOtp},
        commands::VerifyYubikeyCommand,
        piv::validate_piv_pin_len,
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
    let local_verify = (|| {
        use crate::secrets::domain::piv::SecretName;
        let mut bw_email: Option<ProtectedSecret> = None;
        let mut bw_password: Option<ProtectedSecret> = None;
        let mut bws_access_token: Option<ProtectedSecret> = None;
        for storage in SecretStorageVerificationPlan::for_serial(serial).into_targets() {
            let name = storage.name;
            let inspection = storage_port.inspect_secret_storage_read(serial, &storage)?;
            let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
            let secret = storage_port
                .load_secret(serial, &intent, pin.as_ref())
                .map_err(|error| intent.decode_error(error))?;
            intent.validate_loaded_secret(&secret)?;
            match name {
                SecretName::BwEmail => bw_email = Some(secret),
                SecretName::BwPassword => bw_password = Some(secret),
                SecretName::BwsAccessToken => bws_access_token = Some(secret),
            }
        }
        Ok((bw_email, bw_password, bws_access_token))
    })();
    let (loaded_bw_email, loaded_bw_password, loaded_bws_access_token) = match local_verify {
        Ok(value) => value,
        Err(err) => {
            return report
                .write_verify_report(&VerifySummary::local_storage_failed(serial))
                .and(Err(err));
        }
    };
    if requested.is_empty() {
        return report.write_verify_report(&VerifySummary::local_storage_verified(serial));
    }
    let access_token = loaded_bws_access_token.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "internal invariant violated: verification plan did not yield bws-access-token"
        )
    })?;
    let mut summary = VerifySummary::local_storage_verified(serial);
    let mut first_error = None;
    for check in requested {
        match check {
            CheckName::Bws => {
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
                // bw-login 外部確認は、YubiKey 由来の `bw-email` / `bw-password` と入力 OTP で
                // 実際に `bw login` / `bw unlock` の到達性を確認する（spec L107）。bw-login use case と
                // 同じ実行経路（`BwLoginPort`）を再利用し、master password は port の `BW_PASSWORD` env 境界で
                // だけ子プロセスへ渡る。session key は確認専用のため surface せず破棄する。
                match run_bw_login_check(
                    loaded_bw_email.as_ref(),
                    loaded_bw_password.as_ref(),
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
/// local storage 検証で load 済みの `bw-email` / `bw-password` を使い、OTP を入力して `bw login` / `bw unlock`
/// の到達性を確認する。email は protection 境界の内側で argv 安全な値へ翻訳し、master password は port へ保護値
/// として渡す。session key は確認専用のため受け取った値を surface せず破棄し、login / unlock の成否だけを返す。
async fn run_bw_login_check<O, L>(
    bw_email: Option<&ProtectedSecret>,
    bw_password: Option<&ProtectedSecret>,
    otp_input: &O,
    bw_login_port: &L,
) -> Result<()>
where
    O: ports::BwOtpInputPort,
    L: ports::BwLoginPort,
{
    let bw_email = bw_email.ok_or_else(|| {
        anyhow::anyhow!("internal invariant violated: verification plan did not yield bw-email")
    })?;
    let bw_password = bw_password.ok_or_else(|| {
        anyhow::anyhow!("internal invariant violated: verification plan did not yield bw-password")
    })?;
    let email: BwLoginEmail = bw_login::parse_email(bw_email)?;
    let otp = BwOtp::parse(&otp_input.read_bw_otp()?)?;
    bw_login_port
        .login_and_unlock(&email, bw_password, &otp)
        .await
        .map(|_session| ())
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
            storage
                .expect_inspect_secret_storage_read()
                .times(1)
                .in_sequence(sequence)
                .withf(move |actual_serial, storage| {
                    *actual_serial == serial && storage.name == name
                })
                .returning(|_, _| Ok(read_inspection()));
            storage
                .expect_load_secret()
                .times(1)
                .in_sequence(sequence)
                .withf(move |actual_serial, intent, _| {
                    *actual_serial == serial && intent.storage.name == name
                })
                .returning(move |_, intent, _| {
                    Ok(match intent.storage.name {
                        SecretName::BwEmail => material(b"email"),
                        SecretName::BwPassword => material(b"password"),
                        SecretName::BwsAccessToken => material(b"access-token"),
                    })
                });
        }
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
}
