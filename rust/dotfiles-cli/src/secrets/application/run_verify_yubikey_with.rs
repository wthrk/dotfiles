//! verify-yubikey の device 解決順序を固定し、外部検証の責務境界を application に維持する。

use crate::Result;
use crate::secrets::{
    domain::{
        command::VerifyYubikeyCommand,
        piv::validate_piv_pin_len,
        storage::{SecretStorageReadIntent, SecretStorageVerificationPlan},
        summary::{CheckName, CheckStatus, VerifySummary},
    },
    ports,
};

/// 保存済み secret の存在と、要求された外部確認項目を検証する。
///
/// serial 未指定時の自動選択を device port 境界へ委譲し、local storage 検証を完了条件の
/// 先頭に固定する。外部確認結果は report 境界へ明示的に反映し、verify 結果の責任範囲を
/// 曖昧にしない。
pub(crate) async fn run_verify_yubikey_with<D, P, S, R, B>(
    command: VerifyYubikeyCommand,
    device_serial: &mut D,
    pin_policy: &mut impl ports::yubikey::DevicePinPolicyPort,
    process: &P,
    storage_port: &mut S,
    report: &R,
    bws_client: &B,
) -> Result<()>
where
    D: ports::yubikey::DeviceSerialPort,
    P: ports::io::PinInputPort,
    S: ports::yubikey::SecretStoragePort,
    R: ports::io::ReportPort,
    B: ports::bw::BwsClientPort,
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
        let mut bws_access_token = None;
        for storage in SecretStorageVerificationPlan::for_serial(serial).into_targets() {
            let is_bws_access_token =
                storage.name == crate::secrets::domain::piv::SecretName::BwsAccessToken;
            let inspection = storage_port.inspect_secret_storage_read(serial, &storage)?;
            let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
            let secret = storage_port
                .load_secret(serial, &intent, pin.as_ref())
                .map_err(|error| intent.decode_error(error))?;
            intent.validate_loaded_secret(&secret)?;
            if is_bws_access_token {
                bws_access_token = Some(secret);
            }
        }
        Ok(bws_access_token)
    })();
    let bws_access_token = match local_verify {
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
    let access_token = bws_access_token.ok_or_else(|| {
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
                        .list_bws_projects(&access_token)
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
                let secret_candidates = match bws_client
                    .list_bws_secrets(&access_token, &project_id)
                    .await
                {
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
                        .fetch_bws_secret_by_id(&access_token, &secret_id)
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
                summary.mark_external_check(CheckName::BwLogin, CheckStatus::Failed);
                if first_error.is_none() {
                    first_error = Some(anyhow::anyhow!(
                        "external checks are not implemented yet: {}",
                        CheckName::BwLogin.as_str()
                    ));
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

#[cfg(test)]
mod tests {
    use crate::secrets::{
        domain::{
            bws::{BwsLookupCandidate, BwsProjectId, BwsSecretId},
            command::{ExternalCheck, VerifyYubikeyCommand},
            manifest::SecretManifest,
            piv::SecretName,
            storage::SecretStorageReadInspection,
            summary::{CheckName, CheckStatus},
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
        storage: &mut ports::yubikey::MockSecretStoragePort,
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
        let mut device_serial = ports::yubikey::MockDeviceSerialPort::new();
        let mut pin_policy = ports::yubikey::MockDevicePinPolicyPort::new();
        device_serial.expect_resolve_device_serial().times(0);
        let process = ports::io::MockPinInputPort::new();
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        storage.expect_inspect_secret_storage_read().times(0);
        let report = ports::io::MockReportPort::new();
        let bws = ports::bw::MockBwsClientPort::new();

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
        )
        .await;

        assert!(result.is_err(), "--all and --check cannot be used together");
    }

    #[tokio::test]
    async fn verify_bws_check_fetches_required_secrets_and_reports_ok() -> crate::Result<()> {
        let mut device_serial = ports::yubikey::MockDeviceSerialPort::new();
        let mut pin_policy = ports::yubikey::MockDevicePinPolicyPort::new();
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

        let process = ports::io::MockPinInputPort::new();
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence, 2001);

        let mut bws = ports::bw::MockBwsClientPort::new();
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

        let mut report = ports::io::MockReportPort::new();
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
        )
        .await
    }

    #[tokio::test]
    async fn verify_bws_check_reports_project_lookup_failure() {
        let mut device_serial = ports::yubikey::MockDeviceSerialPort::new();
        let mut pin_policy = ports::yubikey::MockDevicePinPolicyPort::new();
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
        let process = ports::io::MockPinInputPort::new();
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence, 2001);
        let mut bws = ports::bw::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(Vec::new()));
        bws.expect_list_bws_secrets().times(0);
        bws.expect_fetch_bws_secret_by_id().times(0);
        let mut report = ports::io::MockReportPort::new();
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
        )
        .await;

        assert!(result.is_err(), "missing BWS project must fail");
    }

    #[tokio::test]
    async fn verify_bws_check_reports_secret_lookup_failure() {
        let mut device_serial = ports::yubikey::MockDeviceSerialPort::new();
        let mut pin_policy = ports::yubikey::MockDevicePinPolicyPort::new();
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
        let process = ports::io::MockPinInputPort::new();
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence, 2001);
        let mut bws = ports::bw::MockBwsClientPort::new();
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
        let mut report = ports::io::MockReportPort::new();
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
        )
        .await;

        assert!(result.is_err(), "missing BWS secret must fail");
    }

    #[tokio::test]
    async fn verify_bws_check_reports_fetch_failure() {
        let mut device_serial = ports::yubikey::MockDeviceSerialPort::new();
        let mut pin_policy = ports::yubikey::MockDevicePinPolicyPort::new();
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
        let process = ports::io::MockPinInputPort::new();
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence, 2001);
        let mut bws = ports::bw::MockBwsClientPort::new();
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
        let mut report = ports::io::MockReportPort::new();
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
        )
        .await;

        assert!(result.is_err(), "BWS fetch failure must fail");
    }
}
