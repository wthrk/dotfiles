//! restore-gpg の鍵リング復元順序を固定し、GPG/SSH の low-level 操作を port 境界の外へ閉じる。

use crate::Result;
use crate::secrets::{
    domain::{
        bws::{BwsProjectName, BwsSecretName},
        commands::RestoreGpgCommand,
        gpg_restore::RestoreGpgSummary,
        piv::{SecretName, validate_piv_pin_len},
        storage::SecretStorageReadIntent,
    },
    ports,
};

/// `gpg-secret-key-backup` encrypted envelope を接続中 YubiKey で復号して鍵リングへ復元する。
///
/// 設計「鍵リング復元契約」の 10 ステップを順序制御として固定する。envelope 検証・recipient 照合・
/// fingerprint 一致・既存鍵衝突・subkey 利用可能・gpg-agent SSH support 充足のいずれかで停止条件に
/// 達した場合は、後続の SSH 公開鍵経路へ進ませない。secret key material・DEK・復号済み backup は
/// すべて port 境界の保護値として扱い、application 層では加工しない。順序を application に固定するのは、
/// 「import 前に fingerprint を確定し既存鍵衝突を止める」「subkey 検証成功まで SSH 経路へ進ませない」
/// という停止条件の責務境界を保護するためである。
#[expect(
    clippy::too_many_arguments,
    reason = "restore-gpg は device/pin/storage/bws/recipient/cipher/keyring/ssh-agent/report の port を順序適用する単一 use case"
)]
pub(crate) async fn run_restore_gpg<D, P, S, B, Y, C, K, A, R>(
    command: RestoreGpgCommand,
    device_serial: &mut D,
    pin_policy: &mut impl ports::DevicePinPolicyPort,
    process: &P,
    storage_port: &mut S,
    bws_client: &B,
    recipient: &mut Y,
    cipher: &mut C,
    keyring: &mut K,
    ssh_agent: &mut A,
    report: &R,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    P: ports::PinInputPort,
    S: ports::SecretStoragePort,
    B: ports::BwsClientPort,
    Y: ports::GpgRecipientPort,
    C: ports::BackupCipherPort,
    K: ports::GpgKeyringPort,
    A: ports::SshAgentPort,
    R: ports::ReportPort,
{
    let serial = device_serial.resolve_device_serial(command.serial)?;
    let pin = if pin_policy.device_requires_pin(serial)? {
        let pin = process.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };

    // 1-2. bws-access-token を YubiKey storage から読み出し、BWS から envelope を取得する。
    let access_token = load_bws_access_token(serial, storage_port, pin.as_ref())?;
    let project_id = BwsProjectName::DOTFILES_SECRET_RECOVERY
        .resolve_id(bws_client.list_bws_projects(&access_token).await?)?;
    let secret_id = BwsSecretName::GpgSecretKeyBackup.resolve_id(
        bws_client
            .list_bws_secrets(&access_token, &project_id)
            .await?,
        &project_id,
    )?;

    // 3. envelope 形式（version / metadata / recipients / ciphertext）を検証して取得する。
    let (envelope, _guard) = bws_client
        .fetch_gpg_backup_envelope(&access_token, &secret_id)
        .await?;

    // 3-4. 接続中 YubiKey に一致する recipient を解決し、DEK を unwrap して backup を復号する。
    let connected = recipient.resolve_connected_recipient(serial)?;
    let matched = envelope.resolve_recipient(&connected)?;
    let dek = recipient.unwrap_dek(serial, matched, pin.as_ref())?;
    let backup = cipher.decrypt_backup(&dek, envelope.ciphertext())?;

    // 5. 復号済み backup から primary fingerprint を導出し、metadata と一致を検証する。
    let parsed_fingerprint = keyring.parse_backup_primary_fingerprint(&backup)?;
    if parsed_fingerprint.as_str() != envelope.metadata().primary_fingerprint().as_str() {
        anyhow::bail!(
            "decrypted gpg backup primary fingerprint does not match the envelope metadata"
        );
    }

    // 6. 同一 primary fingerprint の secret key が既に鍵リングにある場合は停止する。
    if keyring.secret_key_exists(&parsed_fingerprint)? {
        anyhow::bail!(
            "a GPG secret key with this primary fingerprint already exists; refusing to import"
        );
    }

    // 7-8. import し、import 後鍵の subkey 構成を検証する。
    let imported = keyring.import_secret_key(&backup)?;
    keyring.inspect_imported_key(&imported)?.ensure_usable()?;

    // 9. authentication subkey の keygrip を gpg-agent の SSH key list へ登録する（冪等）。
    let keygrip = keyring.authentication_subkey_keygrip(&imported)?;
    ssh_agent.register_authentication_subkey(&keygrip)?;

    // 10. gpg-agent SSH support 利用可否を確認する。
    ssh_agent.inspect_ssh_agent(&keygrip)?.ensure_ready()?;

    report.write_restore_gpg_report(&RestoreGpgSummary {
        primary_fingerprint: imported.as_str().to_owned(),
        ssh_key_registered: true,
        ssh_support_ready: true,
    })
}

/// bws-access-token を YubiKey storage の read 経路（inspect → intent → load → validate）で取得する。
fn load_bws_access_token<S>(
    serial: u32,
    storage_port: &mut S,
    pin: Option<&crate::secrets::support::protection::ProtectedSecret>,
) -> Result<crate::secrets::support::protection::ProtectedSecret>
where
    S: ports::SecretStoragePort,
{
    let storage = SecretName::BwsAccessToken.storage_spec(serial);
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
    //! restore-gpg の順序制御と停止条件を mockall + Sequence で検証する単体テスト。
    //!
    //! 鍵リング backend / SSH agent backend / recipient backend / cipher backend を port mock で差し替え、
    //! envelope 取得→recipient 照合→DEK unwrap→backup 復号→fingerprint 照合→既存鍵衝突→import→subkey
    //! 検証→keygrip 登録→SSH support 充足という順序と、各停止条件を検証する。test double は持ち込まない。

    use crate::secrets::{
        domain::{
            commands::RestoreGpgCommand,
            gpg_backup::{BackupUpdateGuard, ConnectedYubiKey, GpgBackupEnvelope},
            gpg_restore::{
                ImportedKeyComposition, Keygrip, ResolvedSubkey, SshAgentReadiness,
                SubkeyCapability,
            },
            manifest::SecretManifest,
            storage::SecretStorageReadInspection,
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_restore_gpg;

    const PRIMARY_FP: &str = "0123456789abcdef0123456789abcdef01234567";
    const KEYGRIP: &str = "AABBCCDDEEFF00112233445566778899AABBCCDD";

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn read_inspection() -> SecretStorageReadInspection {
        SecretStorageReadInspection {
            manifest_bytes: Some(SecretManifest::expected().encode().expect("manifest")),
            encoded: Some(vec![1]),
        }
    }

    /// serial 2001 に一致する recipient を 1 件持つ有効 envelope JSON を作る。
    fn envelope() -> GpgBackupEnvelope {
        // public_key_fingerprint は recipient mock が返す ConnectedYubiKey と一致させる。
        let pubkey = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let json = format!(
            r#"{{
              "version": 1,
              "metadata": {{
                "primary_fingerprint": "{PRIMARY_FP}",
                "exported_at": "2026-05-31T00:00:00Z",
                "dek_alg": "aes-256-gcm",
                "recipient_kek_alg": "rsa-oaep-sha256"
              }},
              "recipients": [
                {{
                  "yubikey_serial": "2001",
                  "piv_slot": "82",
                  "public_key_fingerprint": "{pubkey}",
                  "wrapped_dek": "d3JhcHBlZA=="
                }}
              ],
              "ciphertext": {{
                "nonce": "EBESExQVFhcYGRob",
                "body": "ZW5jcnlwdGVk",
                "tag": "gIGCg4SFhoeIiYqLjI2Ojw=="
              }}
            }}"#
        );
        GpgBackupEnvelope::parse(&json).expect("valid envelope")
    }

    fn connected() -> ConnectedYubiKey {
        ConnectedYubiKey::new(
            "2001",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .expect("connected yubikey")
    }

    fn all_usable_composition() -> ImportedKeyComposition {
        ImportedKeyComposition::new(
            true,
            vec![
                ResolvedSubkey {
                    capability: SubkeyCapability::Encryption,
                    usable: true,
                },
                ResolvedSubkey {
                    capability: SubkeyCapability::Authentication,
                    usable: true,
                },
                ResolvedSubkey {
                    capability: SubkeyCapability::Signing,
                    usable: true,
                },
            ],
        )
    }

    fn expect_local_storage_ok(
        storage: &mut ports::MockSecretStoragePort,
        sequence: &mut mockall::Sequence,
    ) {
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .in_sequence(sequence)
            .returning(|_, _| Ok(read_inspection()));
        storage
            .expect_load_secret()
            .times(1)
            .in_sequence(sequence)
            .returning(|_, _, _| Ok(material(b"access-token")));
    }

    #[tokio::test]
    async fn restore_gpg_runs_full_order_and_reports() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|requested| Ok(requested.expect("serial")));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(false));
        let process = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence);

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().times(1).returning(|_| {
            Ok(vec![crate::secrets::domain::bws::BwsLookupCandidate {
                id: crate::secrets::domain::bws::BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        bws.expect_list_bws_secrets().times(1).returning(|_, _| {
            Ok(vec![crate::secrets::domain::bws::BwsLookupCandidate {
                id: crate::secrets::domain::bws::BwsSecretId::new("gpg-id"),
                name: "gpg-secret-key-backup".to_owned(),
            }])
        });
        bws.expect_fetch_gpg_backup_envelope()
            .times(1)
            .returning(|_, _| Ok((envelope(), BackupUpdateGuard::ValueDigest("d".to_owned()))));

        let mut recipient = ports::MockGpgRecipientPort::new();
        recipient
            .expect_resolve_connected_recipient()
            .times(1)
            .returning(|_| Ok(connected()));
        recipient
            .expect_unwrap_dek()
            .times(1)
            .returning(|_, _, _| Ok(material(b"dek")));

        let mut cipher = ports::MockBackupCipherPort::new();
        cipher
            .expect_decrypt_backup()
            .times(1)
            .returning(|_, _| Ok(material(b"backup")));

        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_parse_backup_primary_fingerprint()
            .times(1)
            .returning(|_| {
                crate::secrets::domain::gpg_backup::PrimaryFingerprint::parse(PRIMARY_FP)
            });
        keyring
            .expect_secret_key_exists()
            .times(1)
            .returning(|_| Ok(false));
        keyring.expect_import_secret_key().times(1).returning(|_| {
            crate::secrets::domain::gpg_backup::PrimaryFingerprint::parse(PRIMARY_FP)
        });
        keyring
            .expect_inspect_imported_key()
            .times(1)
            .returning(|_| Ok(all_usable_composition()));
        keyring
            .expect_authentication_subkey_keygrip()
            .times(1)
            .returning(|_| Keygrip::parse(KEYGRIP));

        let mut ssh_agent = ports::MockSshAgentPort::new();
        ssh_agent
            .expect_register_authentication_subkey()
            .times(1)
            .returning(|_| Ok(()));
        ssh_agent
            .expect_inspect_ssh_agent()
            .times(1)
            .returning(|_| {
                Ok(SshAgentReadiness {
                    socket_resolved: true,
                    authentication_identity_present: true,
                })
            });

        let mut report = ports::MockReportPort::new();
        report
            .expect_write_restore_gpg_report()
            .times(1)
            .withf(|summary| {
                summary.primary_fingerprint == PRIMARY_FP
                    && summary.ssh_key_registered
                    && summary.ssh_support_ready
            })
            .returning(|_| Ok(()));

        run_restore_gpg(
            RestoreGpgCommand { serial: Some(2001) },
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &bws,
            &mut recipient,
            &mut cipher,
            &mut keyring,
            &mut ssh_agent,
            &report,
        )
        .await
    }

    #[tokio::test]
    async fn restore_gpg_stops_when_existing_key_collides() {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .returning(|_| Ok(false));
        let process = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_read()
            .returning(|_, _| Ok(read_inspection()));
        storage
            .expect_load_secret()
            .returning(|_, _, _| Ok(material(b"access-token")));
        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().returning(|_| {
            Ok(vec![crate::secrets::domain::bws::BwsLookupCandidate {
                id: crate::secrets::domain::bws::BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![crate::secrets::domain::bws::BwsLookupCandidate {
                id: crate::secrets::domain::bws::BwsSecretId::new("gpg-id"),
                name: "gpg-secret-key-backup".to_owned(),
            }])
        });
        bws.expect_fetch_gpg_backup_envelope()
            .returning(|_, _| Ok((envelope(), BackupUpdateGuard::ValueDigest("d".to_owned()))));
        let mut recipient = ports::MockGpgRecipientPort::new();
        recipient
            .expect_resolve_connected_recipient()
            .returning(|_| Ok(connected()));
        recipient
            .expect_unwrap_dek()
            .returning(|_, _, _| Ok(material(b"dek")));
        let mut cipher = ports::MockBackupCipherPort::new();
        cipher
            .expect_decrypt_backup()
            .returning(|_, _| Ok(material(b"backup")));
        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_parse_backup_primary_fingerprint()
            .returning(|_| {
                crate::secrets::domain::gpg_backup::PrimaryFingerprint::parse(PRIMARY_FP)
            });
        keyring.expect_secret_key_exists().returning(|_| Ok(true));
        // 既存鍵衝突で import へ進ませない。
        keyring.expect_import_secret_key().times(0);
        let mut ssh_agent = ports::MockSshAgentPort::new();
        ssh_agent.expect_register_authentication_subkey().times(0);
        let report = ports::MockReportPort::new();

        let result = run_restore_gpg(
            RestoreGpgCommand { serial: Some(2001) },
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &bws,
            &mut recipient,
            &mut cipher,
            &mut keyring,
            &mut ssh_agent,
            &report,
        )
        .await;

        assert!(result.is_err(), "existing key collision must stop import");
    }
}
