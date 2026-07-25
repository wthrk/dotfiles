//! verify-yubikey の device 解決順序を固定し、外部検証の責務境界を application に維持する。

use crate::{
    Result,
    features::provisioning_verification::domain::commands::VerifyYubikeyCommand,
    features::{
        bws_secrets::ports::public::{BwsClientPort, BwsProjectName, BwsSecretName},
        cli_interaction::ports::public::ReportPort,
        gpg_backup_recovery::ports::public::GpgBackupEnvelope,
        password_store::ports::public::PasswordStoreRemote,
        provisioning_verification::domain::verification::{CheckName, CheckStatus, VerifySummary},
        yubikey_lifecycle::ports::public::{
            DeviceSerialPort, GpgRecipientPort, SecretName, SecretStoragePort,
            SecretStorageReadIntent, SecretStorageVerificationPlan,
        },
    },
    foundation::protection::ProtectedSecret,
};

/// 保存済み secret の存在と、要求された外部確認項目を検証する。
///
/// serial 未指定時の自動選択を device port 境界へ委譲し、local storage 検証を完了条件の
/// 先頭に固定する。外部確認結果は report 境界へ明示的に反映し、verify 結果の責任範囲を
/// 曖昧にしない。
pub(crate) async fn run_verify_yubikey_with<B>(
    command: VerifyYubikeyCommand,
    device: &mut dyn DeviceSerialPort,
    storage_port: &mut dyn SecretStoragePort,
    report: &dyn ReportPort,
    bws_client: &B,
    gpg_recipient: &mut dyn GpgRecipientPort,
) -> Result<()>
where
    B: BwsClientPort + ?Sized,
{
    let requested = command.requested_external_checks()?;
    let serial = device.resolve_device_serial(command.serial)?;
    // local storage 検証は、無対話 BWS recovery に必要な `bitwarden-client-secret` だけを
    // inspect → intent → load → validate する。
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
            CheckName::Setup | CheckName::BitwardenClientSecret | CheckName::LocalStorage => {
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
    gpg_recipient: &mut dyn GpgRecipientPort,
) -> Result<()>
where
    B: BwsClientPort + ?Sized,
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

    let (raw_envelope, _guard) = bws_client
        .fetch_gpg_backup_envelope(access_token, &gpg_secret_id)
        .await?;
    let envelope = GpgBackupEnvelope::from_json(raw_envelope.as_bytes())?;
    let connected = gpg_recipient.resolve_connected_recipient(serial)?;
    envelope.resolve_recipient(&connected)?;

    let raw_remote = bws_client
        .fetch_password_store_remote(access_token, &pass_secret_id)
        .await?;
    let _remote = String::from_utf8(raw_remote.as_bytes().to_vec())
        .map_err(|_| anyhow::anyhow!("password-store-remote is not valid UTF-8"))
        .and_then(|value| PasswordStoreRemote::parse(&value))?;
    Ok(())
}

/// YubiKey storage の read 経路（inspect → intent → load → validate）で指定 secret を on-demand 取得する。
///
/// この helper は application 層の順序制御（必要な分岐の直前で必要 secret だけを読み、戻ると drop する）に
/// 属するため `pub` にしない。read 操作は追加の入力を要求しない。
/// BWS external check の read-only helper として、この use case 内に閉じる。
fn load_yubikey_secret(
    serial: u32,
    name: SecretName,
    storage_port: &mut dyn SecretStoragePort,
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
    use crate::features::bws_secrets::ports::public::BwsSecretValue;
    use crate::{
        features::{
            bws_secrets::ports::public::{BwsLookupCandidate, BwsProjectId, BwsSecretId},
            gpg_backup_recovery::ports::public::{
                BackupUpdateGuard, ConnectedYubiKey, GpgBackupEnvelope,
            },
            provisioning_verification::domain::{
                commands::VerifyYubikeyCommand,
                verification::{CheckName, CheckStatus, ExternalCheck},
            },
            yubikey_lifecycle::ports::public::{
                SecretManifest, SecretName, SecretStorageReadInspection,
            },
        },
        foundation::protection::ProtectedSecret,
    };

    mod ports {
        pub(crate) use crate::features::bws_secrets::ports::public::MockBwsClientPort;
        pub(crate) use crate::features::cli_interaction::ports::public::MockReportPort;
        pub(crate) use crate::features::yubikey_lifecycle::ports::public::MockGpgRecipientPort;
    }

    use super::{run_bws_check, run_verify_yubikey_with};

    fn material(bytes: &[u8]) -> crate::Result<ProtectedSecret> {
        ProtectedSecret::from_test_bytes(bytes)
    }

    fn read_inspection() -> crate::Result<SecretStorageReadInspection> {
        Ok(SecretStorageReadInspection {
            manifest_bytes: Some(SecretManifest::fixture_v2().encode()?),
            encoded: Some(vec![1]),
        })
    }

    /// secret 名から決め打ちの test 値を返す。on-demand ロードと local 検証で同じ値を返す。
    fn secret_value(name: SecretName) -> crate::Result<ProtectedSecret> {
        match name {
            SecretName::BitwardenClientSecret => material(b"client-secret"),
        }
    }

    fn valid_envelope() -> crate::Result<GpgBackupEnvelope> {
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
    }

    fn connected_recipient() -> crate::Result<ConnectedYubiKey> {
        ConnectedYubiKey::new(
            "2001",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
    }

    fn valid_envelope_value() -> crate::Result<String> {
        valid_envelope()?.to_json_string()
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
        storage: &mut crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort,
        sequence: &mut mockall::Sequence,
        serial: u32,
    ) {
        expect_secret_load(storage, sequence, serial, SecretName::BitwardenClientSecret);
    }

    /// 指定 secret の inspect → load を 1 回ずつ順序付きで期待する（on-demand ロード 1 回分）。
    fn expect_secret_load(
        storage: &mut crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort,
        sequence: &mut mockall::Sequence,
        serial: u32,
        name: SecretName,
    ) {
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .in_sequence(sequence)
            .withf(move |actual_serial, storage| *actual_serial == serial && storage.name == name)
            .returning(|_, _| read_inspection());
        storage
            .expect_load_secret()
            .times(1)
            .in_sequence(sequence)
            .withf(move |actual_serial, intent| {
                *actual_serial == serial && intent.storage.name == name
            })
            .returning(move |_, intent| secret_value(intent.storage.name));
    }

    #[tokio::test]
    async fn verify_rejects_conflicting_external_check_flags_before_ports() {
        let mut device_serial =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        device_serial.expect_resolve_device_serial().times(0);
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
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
            &mut device_serial,
            &mut storage,
            &report,
            &bws,
            &mut gpg_recipient,
        )
        .await;

        assert!(result.is_err(), "--all and --check cannot be used together");
    }

    #[tokio::test]
    async fn verify_bws_check_fetches_required_secrets_and_reports_ok() -> crate::Result<()> {
        let mut device_serial =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        let mut sequence = mockall::Sequence::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|requested| {
                requested.ok_or_else(|| anyhow::anyhow!("test requires an explicit serial"))
            });
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence, 2001);
        // BWS 分岐は bitwarden-client-secret を on-demand にロードする。
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
                    BwsSecretValue::from_bytes(valid_envelope_value()?.as_bytes().to_vec()),
                    BackupUpdateGuard::from_value_bytes(b"envelope"),
                ))
            });
        gpg_recipient
            .expect_resolve_connected_recipient()
            .times(1)
            .withf(|serial| *serial == 2001)
            .returning(|_| connected_recipient());
        gpg_recipient.expect_unwrap_dek().times(0);
        bws.expect_fetch_password_store_remote()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|_, secret_id| secret_id.as_str() == "pass-id")
            .returning(|_, _| {
                Ok(BwsSecretValue::from_bytes(
                    "git@github.com:owner/password-store.git"
                        .as_bytes()
                        .to_vec(),
                ))
            });

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
            &mut storage,
            &report,
            &bws,
            &mut gpg_recipient,
        )
        .await
    }

    #[tokio::test]
    async fn bws_check_fails_when_gpg_backup_schema_is_invalid() -> crate::Result<()> {
        let access_token = material(b"access-token")?;
        let mut bws = ports::MockBwsClientPort::new();
        expect_bws_lookup(&mut bws);
        bws.expect_fetch_gpg_backup_envelope()
            .times(1)
            .returning(|_, _| Err(anyhow::anyhow!("invalid gpg backup envelope response")));
        bws.expect_fetch_password_store_remote().times(0);
        let mut gpg_recipient = ports::MockGpgRecipientPort::new();
        gpg_recipient.expect_resolve_connected_recipient().times(0);

        let result = run_bws_check(2001, &access_token, &bws, &mut gpg_recipient).await;

        assert!(
            result.is_err(),
            "invalid envelope schema must fail bws check"
        );
        Ok(())
    }

    #[tokio::test]
    async fn bws_check_fails_when_primary_fingerprint_is_not_lowercase_hex_40() -> crate::Result<()>
    {
        let access_token = material(b"access-token")?;
        let mut bws = ports::MockBwsClientPort::new();
        expect_bws_lookup(&mut bws);
        bws.expect_fetch_gpg_backup_envelope()
            .times(1)
            .returning(|_, _| Err(anyhow::anyhow!("invalid gpg backup envelope response")));
        bws.expect_fetch_password_store_remote().times(0);
        let mut gpg_recipient = ports::MockGpgRecipientPort::new();
        gpg_recipient.expect_resolve_connected_recipient().times(0);

        let result = run_bws_check(2001, &access_token, &bws, &mut gpg_recipient).await;

        assert!(
            result.is_err(),
            "invalid primary fingerprint must fail bws check"
        );
        Ok(())
    }

    #[tokio::test]
    async fn bws_check_fails_when_connected_yubikey_recipient_does_not_match() -> crate::Result<()>
    {
        let access_token = material(b"access-token")?;
        let mut bws = ports::MockBwsClientPort::new();
        expect_bws_lookup(&mut bws);
        bws.expect_fetch_gpg_backup_envelope()
            .times(1)
            .returning(|_, _| {
                Ok((
                    BwsSecretValue::from_bytes(valid_envelope_value()?.as_bytes().to_vec()),
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
        Ok(())
    }

    #[tokio::test]
    async fn bws_check_fails_when_unwrap_free_recoverability_cannot_be_established()
    -> crate::Result<()> {
        let access_token = material(b"access-token")?;
        let mut bws = ports::MockBwsClientPort::new();
        expect_bws_lookup(&mut bws);
        bws.expect_fetch_gpg_backup_envelope()
            .times(1)
            .returning(|_, _| {
                Ok((
                    BwsSecretValue::from_bytes(valid_envelope_value()?.as_bytes().to_vec()),
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
        Ok(())
    }

    #[tokio::test]
    async fn verify_bws_check_reports_project_lookup_failure() {
        let mut device_serial =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        let mut sequence = mockall::Sequence::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence, 2001);
        // BWS 分岐は bitwarden-client-secret を on-demand にロードする。
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
            &mut device_serial,
            &mut storage,
            &report,
            &bws,
            &mut gpg_recipient,
        )
        .await;

        assert!(result.is_err(), "missing BWS project must fail");
    }

    #[tokio::test]
    async fn verify_bws_check_reports_secret_lookup_failure() {
        let mut device_serial =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        let mut sequence = mockall::Sequence::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence, 2001);
        // BWS 分岐は bitwarden-client-secret を on-demand にロードする。
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
            &mut device_serial,
            &mut storage,
            &report,
            &bws,
            &mut gpg_recipient,
        )
        .await;

        assert!(result.is_err(), "missing BWS secret must fail");
    }

    #[tokio::test]
    async fn verify_bws_check_reports_fetch_failure() {
        let mut device_serial =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        let mut sequence = mockall::Sequence::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(2001));
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence, 2001);
        // BWS 分岐は bitwarden-client-secret を on-demand にロードする。
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
            &mut device_serial,
            &mut storage,
            &report,
            &bws,
            &mut gpg_recipient,
        )
        .await;

        assert!(result.is_err(), "BWS fetch failure must fail");
    }
}
