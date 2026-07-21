//! verify-yubikey の device 解決順序を固定し、外部検証の責務境界を application に維持する。

use crate::Result;
use crate::{
    domain::{
        bws::{BwsProjectName, BwsSecretName},
        commands::VerifyYubikeyCommand,
        gpg_backup::GpgBackupEnvelope,
        pass_restore::PasswordStoreRemote,
        piv::SecretName,
        storage::{SecretStorageReadIntent, SecretStorageVerificationPlan},
        verification::{CheckName, CheckStatus, VerifySummary},
    },
    ports,
    support::protection::ProtectedSecret,
};

/// `run_verify_yubikey_with` が使う外部 capability を named field で束ねる。
pub(crate) struct VerifyYubikeyRuntime<'a, B> {
    pub(crate) device: &'a mut dyn ports::DeviceSerialPort,
    pub(crate) storage: &'a mut dyn ports::SecretStoragePort,
    pub(crate) report: &'a dyn ports::ReportPort,
    pub(crate) bws_client: &'a B,
    pub(crate) gpg_recipient: &'a mut dyn ports::GpgRecipientPort,
}

/// 保存済み secret の存在と、要求された外部確認項目を検証する。
///
/// serial 未指定時の自動選択を device port 境界へ委譲し、local storage 検証を完了条件の
/// 先頭に固定する。外部確認結果は report 境界へ明示的に反映し、verify 結果の責任範囲を
/// 曖昧にしない。
pub(crate) async fn run_verify_yubikey_with<B>(
    command: VerifyYubikeyCommand,
    runtime: VerifyYubikeyRuntime<'_, B>,
) -> Result<()>
where
    B: ports::BwsClientPort,
{
    let VerifyYubikeyRuntime {
        device,
        storage: storage_port,
        report,
        bws_client,
        gpg_recipient,
        ..
    } = runtime;
    let requested = command.requested_external_checks()?;
    let serial = device.resolve_device_serial(command.serial)?;
    // local storage 検証は、無対話 BWS recovery に必要な `bitwarden-client-secret` だけを
    // inspect → intent → load → validate する。bw-email / bw-password は Password Manager 用の
    // 任意保存値であり、verify の入力・分岐・成功条件へ混在させない。
    let local_verify = (|| {
        for storage in SecretStorageVerificationPlan::for_serial(serial).into_targets() {
            let inspection = storage_port.inspect_secret_storage_read(serial, &storage)?;
            let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
            let secret = storage_port
                .load_secret(serial, &intent)
                .map_err(|error| intent.decode_error(error))?;
            // BWS recovery prerequisite の 1 値だけを検証する。検証後の secret は retain せず drop する。
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
                // bitwarden-client-secret をこの分岐の中で on-demand にロードし、分岐を抜けると drop されるようにする。
                // BWS 外部確認の `.await` の間だけ access token が future に存在する。
                let access_token = match load_yubikey_secret(
                    serial,
                    SecretName::BitwardenClientSecret,
                    storage_port,
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
                match run_bws_check(serial, access_token, bws_client, gpg_recipient).await {
                    Ok(_) => summary.mark_external_check(CheckName::Bws, CheckStatus::Ok),
                    Err(error) => {
                        summary.mark_external_check(CheckName::Bws, CheckStatus::Failed);
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                    }
                }
            }
            CheckName::Setup
            | CheckName::BwEmail
            | CheckName::BwPassword
            | CheckName::BitwardenClientSecret
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

/// BWS 外部確認で必要な secret 取得・envelope 検証・recipient 照合を順に実行する。
///
/// `gpg-secret-key-backup` は port から opaque value として取得し、application が domain envelope
/// として schema / `metadata.primary_fingerprint` / ciphertext 構造を検証する。
/// application は接続中 YubiKey の recipient identity を取得して domain の matching rule を適用し、
/// unwrap なしで一致 recipient の存在まで確認する。`password-store-remote` も typed port で取得し、
/// raw secret fetch 成功だけを BWS check の成功条件にしない。
async fn run_bws_check<B>(
    serial: u32,
    access_token: &ProtectedSecret,
    bws_client: &B,
    gpg_recipient: &mut dyn ports::GpgRecipientPort,
) -> Result<()>
where
    B: ports::BwsClientPort,
{
    let project_id = BwsProjectName::DOTFILES_SECRET_RECOVERY
        .resolve_id(bws_client.list_bws_projects(access_token).await?)?;
    let secret_candidates = bws_client
        .list_bws_secrets(access_token, &project_id)
        .await?;

    let gpg_secret_id =
        BwsSecretName::GpgSecretKeyBackup.resolve_id(secret_candidates.clone(), &project_id)?;
    let pass_secret_id =
        BwsSecretName::PasswordStoreRemote.resolve_id(secret_candidates, &project_id)?;

    let (envelope_value, _guard) = bws_client
        .fetch_gpg_backup_envelope(access_token, &gpg_secret_id)
        .await?;
    let envelope = GpgBackupEnvelope::from_json(envelope_value.as_bytes())?;
    let connected = gpg_recipient.resolve_connected_recipient(serial)?;
    envelope.resolve_recipient(&connected)?;

    let remote_value = bws_client
        .fetch_password_store_remote(access_token, &pass_secret_id)
        .await?;
    PasswordStoreRemote::parse(&remote_value).map(|_| ())
}

/// YubiKey storage の read 経路（inspect → intent → load → validate）で指定 secret を on-demand 取得する。
///
/// この helper は application 層の順序制御（必要な分岐の直前で必要 secret だけを読み、戻ると drop する）に
/// 属するため `pub` にしない。read 操作は追加の入力を要求しない。
/// BWS external check の read-only helper として、この use case 内に閉じる。
fn load_yubikey_secret(
    serial: u32,
    name: SecretName,
    storage_port: &mut dyn ports::SecretStoragePort,
) -> Result<ProtectedSecret> {
    let storage = name.storage_spec(serial);
    let inspection = storage_port.inspect_secret_storage_read(serial, &storage)?;
    let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
    let secret = storage_port
        .load_secret(serial, &intent)
        .map_err(|error| intent.decode_error(error))?;
    intent.validate_loaded_secret(&secret)?;
    Ok(secret)
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{
            bws::{BwsLookupCandidate, BwsProjectId, BwsSecretId},
            commands::VerifyYubikeyCommand,
            gpg_backup::{BackupUpdateGuard, ConnectedYubiKey, GpgBackupEnvelope},
            manifest::SecretManifest,
            pass_restore::PasswordStoreRemote,
            piv::SecretName,
            storage::SecretStorageReadInspection,
            verification::{CheckName, CheckStatus, ExternalCheck},
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::{VerifyYubikeyRuntime, run_bws_check, run_verify_yubikey_with};

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn read_inspection() -> SecretStorageReadInspection {
        SecretStorageReadInspection {
            manifest_bytes: Some(SecretManifest::fixture_v2().encode().expect("manifest")),
            encoded: Some(vec![1]),
        }
    }

    /// secret 名から決め打ちの test 値を返す。on-demand ロードと local 検証で同じ値を返す。
    fn secret_value(name: SecretName) -> ProtectedSecret {
        match name {
            SecretName::BwEmail => material(b"email"),
            SecretName::BwPassword => material(b"password"),
            SecretName::BitwardenClientSecret => material(b"client-secret"),
        }
    }

    fn valid_envelope() -> GpgBackupEnvelope {
        GpgBackupEnvelope::parse(
            r#"{
              "version": 1,
              "metadata": {
                "primary_fingerprint": "0123456789abcdef0123456789abcdef01234567",
                "exported_at": "2026-05-31T00:00:00Z",
                "dek_alg": "aes-256-gcm",
                "recipient_kek_alg": "rsa-oaep-sha256"
              },
              "recipients": [
                {
                  "yubikey_serial": "2001",
                  "piv_slot": "82",
                  "public_key_fingerprint": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                  "wrapped_dek": "d3JhcHBlZA=="
                }
              ],
              "ciphertext": {
                "nonce": "EBESExQVFhcYGRob",
                "body": "ZW5jcnlwdGVk",
                "tag": "gIGCg4SFhoeIiYqLjI2Ojw=="
              }
            }"#,
        )
        .expect("valid envelope")
    }

    fn connected_recipient() -> ConnectedYubiKey {
        ConnectedYubiKey::new(
            "2001",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .expect("connected recipient")
    }

    fn valid_password_store_remote() -> PasswordStoreRemote {
        PasswordStoreRemote::parse("git@github.com:owner/password-store.git")
            .expect("valid password-store remote")
    }

    fn valid_envelope_value() -> String {
        valid_envelope()
            .to_json_string()
            .expect("serialized envelope")
    }

    fn expect_bws_lookup(bws: &mut ports::MockBwsClientPort) {
        bws.expect_list_bws_projects().times(1).returning(|_| {
            Ok(vec![BwsLookupCandidate {
                id: BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        bws.expect_list_bws_secrets().times(1).returning(|_, _| {
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
    }

    /// local storage 検証（`bitwarden-client-secret` の inspect → load）を順序付きで 1 回期待する。
    ///
    /// 無対話 BWS recovery prerequisite の 1 値だけを inspect/load し、検証後に drop して retain しない。
    fn expect_local_storage_ok(
        storage: &mut ports::MockSecretStoragePort,
        sequence: &mut mockall::Sequence,
        serial: u32,
    ) {
        expect_secret_load(storage, sequence, serial, SecretName::BitwardenClientSecret);
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
            .withf(move |actual_serial, intent| {
                *actual_serial == serial && intent.storage.name == name
            })
            .returning(move |_, intent| Ok(secret_value(intent.storage.name)));
    }

    #[tokio::test]
    async fn verify_rejects_conflicting_external_check_flags_before_ports() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        device_serial.expect_resolve_device_serial().times(0);
        let mut storage = ports::MockSecretStoragePort::new();
        storage.expect_inspect_secret_storage_read().times(0);
        let report = ports::MockReportPort::new();
        let bws = ports::MockBwsClientPort::new();
        let mut gpg_recipient = ports::MockGpgRecipientPort::new();

        let result = run_verify_yubikey_with(
            VerifyYubikeyCommand {
                serial: Some(2001),
                checks: vec![ExternalCheck::Bws],
                all: true,
            },
            VerifyYubikeyRuntime {
                device: &mut device_serial,
                storage: &mut storage,
                report: &report,
                bws_client: &bws,
                gpg_recipient: &mut gpg_recipient,
            },
        )
        .await;

        assert!(result.is_err(), "--all and --check cannot be used together");
    }

    #[tokio::test]
    async fn verify_bws_check_fetches_required_secrets_and_reports_ok() -> crate::Result<()> {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        let mut sequence = mockall::Sequence::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|requested| Ok(requested.expect("serial")));
        let mut storage = ports::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence, 2001);
        // BWS 分岐は bitwarden-client-secret を on-demand ロードする（bw-password はロードしない）。
        expect_secret_load(
            &mut storage,
            &mut sequence,
            2001,
            SecretName::BitwardenClientSecret,
        );

        let mut bws = ports::MockBwsClientPort::new();
        let mut gpg_recipient = ports::MockGpgRecipientPort::new();
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
        bws.expect_fetch_gpg_backup_envelope()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|_, secret_id| secret_id.as_str() == "gpg-id")
            .returning(|_, _| {
                Ok((
                    valid_envelope_value(),
                    BackupUpdateGuard::from_value_bytes(b"envelope"),
                ))
            });
        gpg_recipient
            .expect_resolve_connected_recipient()
            .times(1)
            .withf(|serial| *serial == 2001)
            .returning(|_| Ok(connected_recipient()));
        gpg_recipient.expect_unwrap_dek().times(0);
        bws.expect_fetch_password_store_remote()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|_, secret_id| secret_id.as_str() == "pass-id")
            .returning(|_, _| Ok(valid_password_store_remote().as_str().to_owned()));

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
            VerifyYubikeyRuntime {
                device: &mut device_serial,
                storage: &mut storage,
                report: &report,
                bws_client: &bws,
                gpg_recipient: &mut gpg_recipient,
            },
        )
        .await
    }

    #[tokio::test]
    async fn bws_check_fails_when_gpg_backup_schema_is_invalid() {
        let access_token = material(b"access-token");
        let mut bws = ports::MockBwsClientPort::new();
        expect_bws_lookup(&mut bws);
        bws.expect_fetch_gpg_backup_envelope()
            .times(1)
            .returning(|_, _| {
                Ok((
                    "not-json".to_owned(),
                    BackupUpdateGuard::from_value_bytes(b"not-json"),
                ))
            });
        bws.expect_fetch_password_store_remote().times(0);
        let mut gpg_recipient = ports::MockGpgRecipientPort::new();
        gpg_recipient.expect_resolve_connected_recipient().times(0);

        let result = run_bws_check(2001, &access_token, &bws, &mut gpg_recipient).await;

        assert!(
            result.is_err(),
            "invalid envelope schema must fail bws check"
        );
    }

    #[tokio::test]
    async fn bws_check_fails_when_primary_fingerprint_is_not_lowercase_hex_40() {
        let access_token = material(b"access-token");
        let mut bws = ports::MockBwsClientPort::new();
        expect_bws_lookup(&mut bws);
        bws.expect_fetch_gpg_backup_envelope()
            .times(1)
            .returning(|_, _| {
                let value = r#"{"version":1,"metadata":{"primary_fingerprint":"ABC","exported_at":"2026-05-31T00:00:00Z","dek_alg":"aes-256-gcm","recipient_kek_alg":"rsa-oaep-sha256"},"recipients":[],"ciphertext":{"nonce":"EBESExQVFhcYGRob","body":"ZW5jcnlwdGVk","tag":"gIGCg4SFhoeIiYqLjI2Ojw=="}}"#.to_owned();
                Ok((value, BackupUpdateGuard::from_value_bytes(b"invalid-fingerprint")))
            });
        bws.expect_fetch_password_store_remote().times(0);
        let mut gpg_recipient = ports::MockGpgRecipientPort::new();
        gpg_recipient.expect_resolve_connected_recipient().times(0);

        let result = run_bws_check(2001, &access_token, &bws, &mut gpg_recipient).await;

        assert!(
            result.is_err(),
            "invalid primary fingerprint must fail bws check"
        );
    }

    #[tokio::test]
    async fn bws_check_fails_when_connected_yubikey_recipient_does_not_match() {
        let access_token = material(b"access-token");
        let mut bws = ports::MockBwsClientPort::new();
        expect_bws_lookup(&mut bws);
        bws.expect_fetch_gpg_backup_envelope()
            .times(1)
            .returning(|_, _| {
                Ok((
                    valid_envelope_value(),
                    BackupUpdateGuard::from_value_bytes(b"envelope"),
                ))
            });
        bws.expect_fetch_password_store_remote().times(0);
        let mut gpg_recipient = ports::MockGpgRecipientPort::new();
        gpg_recipient
            .expect_resolve_connected_recipient()
            .times(1)
            .returning(|_| {
                ConnectedYubiKey::new(
                    "2001",
                    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                )
            });
        gpg_recipient.expect_unwrap_dek().times(0);

        let result = run_bws_check(2001, &access_token, &bws, &mut gpg_recipient).await;

        assert!(
            result.is_err(),
            "recipient mismatch must fail before password-store-remote is accepted"
        );
    }

    #[tokio::test]
    async fn bws_check_fails_when_unwrap_free_recoverability_cannot_be_established() {
        let access_token = material(b"access-token");
        let mut bws = ports::MockBwsClientPort::new();
        expect_bws_lookup(&mut bws);
        bws.expect_fetch_gpg_backup_envelope()
            .times(1)
            .returning(|_, _| {
                Ok((
                    valid_envelope_value(),
                    BackupUpdateGuard::from_value_bytes(b"envelope"),
                ))
            });
        bws.expect_fetch_password_store_remote().times(0);
        let mut gpg_recipient = ports::MockGpgRecipientPort::new();
        gpg_recipient
            .expect_resolve_connected_recipient()
            .times(1)
            .returning(|_| anyhow::bail!("connected YubiKey recipient identity is unavailable"));
        gpg_recipient.expect_unwrap_dek().times(0);

        let result = run_bws_check(2001, &access_token, &bws, &mut gpg_recipient).await;

        assert!(
            result.is_err(),
            "bws check must fail when unwrap-free recoverability cannot be established"
        );
    }

    #[tokio::test]
    async fn verify_bws_check_reports_project_lookup_failure() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        let mut sequence = mockall::Sequence::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence, 2001);
        // BWS 分岐は bitwarden-client-secret を on-demand ロードする（bw-password はロードしない）。
        expect_secret_load(
            &mut storage,
            &mut sequence,
            2001,
            SecretName::BitwardenClientSecret,
        );
        let mut bws = ports::MockBwsClientPort::new();
        let mut gpg_recipient = ports::MockGpgRecipientPort::new();
        bws.expect_list_bws_projects()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(Vec::new()));
        bws.expect_list_bws_secrets().times(0);
        bws.expect_fetch_gpg_backup_envelope().times(0);
        bws.expect_fetch_password_store_remote().times(0);
        gpg_recipient.expect_resolve_connected_recipient().times(0);
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
            VerifyYubikeyRuntime {
                device: &mut device_serial,
                storage: &mut storage,
                report: &report,
                bws_client: &bws,
                gpg_recipient: &mut gpg_recipient,
            },
        )
        .await;

        assert!(result.is_err(), "missing BWS project must fail");
    }

    #[tokio::test]
    async fn verify_bws_check_reports_secret_lookup_failure() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        let mut sequence = mockall::Sequence::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence, 2001);
        // BWS 分岐は bitwarden-client-secret を on-demand ロードする（bw-password はロードしない）。
        expect_secret_load(
            &mut storage,
            &mut sequence,
            2001,
            SecretName::BitwardenClientSecret,
        );
        let mut bws = ports::MockBwsClientPort::new();
        let mut gpg_recipient = ports::MockGpgRecipientPort::new();
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
        bws.expect_fetch_gpg_backup_envelope().times(0);
        bws.expect_fetch_password_store_remote().times(0);
        gpg_recipient.expect_resolve_connected_recipient().times(0);
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
            VerifyYubikeyRuntime {
                device: &mut device_serial,
                storage: &mut storage,
                report: &report,
                bws_client: &bws,
                gpg_recipient: &mut gpg_recipient,
            },
        )
        .await;

        assert!(result.is_err(), "missing BWS secret must fail");
    }

    #[tokio::test]
    async fn verify_bws_check_reports_fetch_failure() {
        let mut device_serial = ports::MockDeviceSerialPort::new();
        let mut sequence = mockall::Sequence::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        let mut storage = ports::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence, 2001);
        // BWS 分岐は bitwarden-client-secret を on-demand ロードする（bw-password はロードしない）。
        expect_secret_load(
            &mut storage,
            &mut sequence,
            2001,
            SecretName::BitwardenClientSecret,
        );
        let mut bws = ports::MockBwsClientPort::new();
        let mut gpg_recipient = ports::MockGpgRecipientPort::new();
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
        bws.expect_fetch_gpg_backup_envelope()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Err(anyhow::anyhow!("fetch failed")));
        bws.expect_fetch_password_store_remote().times(0);
        gpg_recipient.expect_resolve_connected_recipient().times(0);
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
            VerifyYubikeyRuntime {
                device: &mut device_serial,
                storage: &mut storage,
                report: &report,
                bws_client: &bws,
                gpg_recipient: &mut gpg_recipient,
            },
        )
        .await;

        assert!(result.is_err(), "BWS fetch failure must fail");
    }
}
