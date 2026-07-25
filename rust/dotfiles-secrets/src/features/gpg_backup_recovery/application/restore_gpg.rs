//! restore-gpg の鍵リング復元順序を固定し、GPG/SSH の low-level 操作を port 境界の外へ閉じる。

use crate::Result;
use crate::{
    features::gpg_backup_recovery::domain::commands::RestoreGpgCommand,
    features::{
        bws_secrets::ports::public::{BwsClientPort, BwsProjectName, BwsSecretName},
        cli_interaction::ports::public::ReportPort,
        gpg_backup_recovery::{
            domain::{
                gpg_backup::{GpgBackupEnvelope, PrimaryFingerprint},
                gpg_restore::RestoreGpgSummary,
            },
            ports::{BackupCipherPort, GpgKeyringPort, SshAgentPort},
        },
        yubikey_lifecycle::ports::public::{
            DeviceSerialPort, GpgRecipientPort, SecretName, SecretStoragePort,
            SecretStorageReadIntent,
        },
    },
    foundation::protection::ProtectedSecret,
};

/// restore-gpg が一回の実行で借用する YubiKey storage capability。
///
/// field は既存 port trait 参照だけであり、recipient 照合、復号、rollback の順序は use case 本体が担う。
pub(crate) struct RestoreGpgYubikeyRuntime<'a> {
    pub(crate) device: &'a mut dyn DeviceSerialPort,
    pub(crate) storage: &'a mut dyn SecretStoragePort,
}

/// restore-gpg が一回の実行で借用する local GnuPG identity capability。
///
/// field は既存 port trait 参照だけであり、import 後の検証・SSH 登録の停止条件をこの bundle は決めない。
pub(crate) struct RestoreGpgIdentityRuntime<'a> {
    pub(crate) keyring: &'a mut dyn GpgKeyringPort,
    pub(crate) ssh_agent: &'a mut dyn SshAgentPort,
}

/// `gpg-secret-key-backup` encrypted envelope を接続中 YubiKey で復号して鍵リングへ復元する。
///
/// 設計「鍵リング復元契約」の 10 ステップを順序制御として固定する。envelope 検証・recipient 照合・
/// fingerprint 一致・既存鍵衝突・subkey 利用可能・gpg-agent SSH support 充足のいずれかで停止条件に
/// 達した場合は、後続の SSH 公開鍵経路へ進ませない。secret key material・DEK・復号済み backup は
/// すべて port 境界の保護値として扱い、application 層では加工しない。順序を application に固定するのは、
/// 「import 前に fingerprint を確定し既存鍵衝突を止める」「subkey 検証成功まで SSH 経路へ進ませない」
/// という停止条件の責務境界を保護するためである。
pub(crate) async fn run_restore_gpg<B>(
    command: RestoreGpgCommand,
    yubikey: RestoreGpgYubikeyRuntime<'_>,
    bws_client: &B,
    recipient: &mut dyn GpgRecipientPort,
    cipher: &mut dyn BackupCipherPort,
    gpg_identity: RestoreGpgIdentityRuntime<'_>,
    report: &dyn ReportPort,
) -> Result<()>
where
    B: BwsClientPort + ?Sized,
{
    let RestoreGpgYubikeyRuntime {
        device,
        storage: storage_port,
    } = yubikey;
    let RestoreGpgIdentityRuntime { keyring, ssh_agent } = gpg_identity;
    let serial = device.resolve_device_serial(command.serial)?;

    // 1-2. bitwarden-client-secret を YubiKey storage から読み出し、BWS から envelope を取得する。
    let access_token = load_bitwarden_client_secret(serial, storage_port)?;
    let project_id = BwsProjectName::DOTFILES_SECRET_RECOVERY
        .resolve_id(bws_client.list_bws_projects(&access_token).await?)?;
    let secret_id = BwsSecretName::GpgSecretKeyBackup.resolve_id(
        bws_client
            .list_bws_secrets(&access_token, &project_id)
            .await?,
        &project_id,
    )?;

    // 3. envelope 形式（version / metadata / recipients / ciphertext）を検証して取得する。
    let (raw_envelope, _guard) = bws_client
        .fetch_gpg_backup_envelope(&access_token, &secret_id)
        .await?;
    let envelope = GpgBackupEnvelope::from_json(raw_envelope.as_bytes())?;

    // 3-4. 接続中 YubiKey に一致する recipient を解決し、DEK を unwrap して backup を復号する。
    let connected = recipient.resolve_connected_recipient(serial)?;
    let matched = envelope.resolve_recipient(&connected)?;
    let dek = recipient.unwrap_dek(serial, matched)?;
    let backup = cipher.decrypt_backup(&dek, envelope.ciphertext())?;

    // 5. 復号済み backup から primary fingerprint を導出し、metadata と一致を検証する。
    let parsed_fingerprint = keyring
        .parse_backup_primary_fingerprint(&backup)?
        .ensure_recovery_capabilities()?;
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

    // 7. import する。import 後の手順 8-10 のいずれかで失敗した場合は、不完全な状態の secret key を
    // 鍵リングに残さず、今回追加した sshcontrol entry も除去してから元エラーを返す。残置すると次回 restore が
    // 手順 6 の既存鍵衝突で止まり復旧不能になるため、import 後の復元処理全体を atomic に扱う。
    let imported = keyring.import_secret_key(&backup)?;
    if imported.as_str() != parsed_fingerprint.as_str() {
        return combine_main_and_cleanup(
            anyhow::anyhow!("GPG import primary fingerprint does not match decrypted backup"),
            keyring.delete_secret_key(&imported),
        );
    }
    match restore_imported_key(&imported, keyring, ssh_agent) {
        Ok(()) => report.write_restore_gpg_report(&RestoreGpgSummary {
            primary_fingerprint: imported.as_str().to_owned(),
            ssh_key_registered: true,
            ssh_support_ready: true,
        }),
        Err(error) => combine_main_and_cleanup(error, keyring.delete_secret_key(&imported)),
    }
}

/// import 後 failure と invocation-owned key の rollback failure の両方を失わずに返す。
///
/// rollback は成功状態への再分類根拠ではない。cleanup 自体が失敗しても主 failure を source chain に残す。
/// cleanup の SDK / process detail は利用者向け diagnostic に昇格させず、固定の非秘匿 category だけを
/// outer error として返す。source chain はデバッグ可能性のため保持するが、presentation はそれを表示しない。
fn combine_main_and_cleanup(main: anyhow::Error, cleanup: Result<()>) -> Result<()> {
    match cleanup {
        Ok(()) => Err(main),
        Err(cleanup) => Err(anyhow::Error::new(RollbackCleanupFailure {
            main,
            _cleanup: cleanup,
        })),
    }
}

#[derive(Debug)]
struct RollbackCleanupFailure {
    main: anyhow::Error,
    // Retained as an opaque carrier: it must not reach the CLI Display boundary.
    _cleanup: anyhow::Error,
}

impl std::fmt::Display for RollbackCleanupFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("GPG restore rollback cleanup failed")
    }
}

impl std::error::Error for RollbackCleanupFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.main.as_ref())
    }
}

/// import 後の subkey 検証・keygrip 登録・SSH support 確認（手順 8-10）を順に実行する。
///
/// import 自体は成功した前提で呼ばれ、ここで返す失敗はすべて呼び出し側で `delete_secret_key` による
/// ロールバック対象になる。手順 8 の subkey 検証に失敗した場合は後続の SSH 経路へ進ませず、手順 9-10 の
/// keygrip 解決 / `sshcontrol` 登録 / SSH support 確認のいずれの失敗も呼び出し側の rollback で原子化する。
/// SSH readiness の失敗時はこの invocation が新設した `sshcontrol` entry だけを先に除去する。
fn restore_imported_key(
    imported: &PrimaryFingerprint,
    keyring: &mut dyn GpgKeyringPort,
    ssh_agent: &mut dyn SshAgentPort,
) -> Result<()> {
    // 8. import 後鍵の subkey 構成（encryption / authentication / signing）を検証する。
    keyring
        .inspect_imported_key(imported)
        .and_then(|composition| composition.ensure_usable())?;

    // 9. authentication subkey の keygrip を gpg-agent の SSH key list へ登録する。既登録 entry は
    // invocation の所有物ではないため、後続失敗時に rollback してはならない。
    let keygrip = keyring.authentication_subkey_keygrip(imported)?;
    let registered_by_invocation = ssh_agent.register_authentication_subkey(&keygrip)?;

    // 10. gpg-agent SSH support 利用可否を確認する。identity 識別は authentication subkey 由来の
    // OpenSSH 公開鍵 key blob を期待値として照合するため、公開鍵を解決して渡す。
    let readiness = (|| {
        let public_key = keyring.authentication_subkey_ssh_public_key(imported)?;
        ssh_agent.inspect_ssh_agent(&public_key)?.ensure_ready()
    })();

    match readiness {
        Ok(()) => Ok(()),
        Err(error) if registered_by_invocation => {
            combine_main_and_cleanup(error, ssh_agent.unregister_authentication_subkey(&keygrip))
        }
        Err(error) => Err(error),
    }
}

/// bitwarden-client-secret を YubiKey storage の read 経路（inspect → intent → load → validate）で取得する。
fn load_bitwarden_client_secret(
    serial: u32,
    storage_port: &mut dyn SecretStoragePort,
) -> Result<ProtectedSecret> {
    let storage = SecretName::BitwardenClientSecret.storage_spec(serial);
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
    //! restore-gpg の順序制御と停止条件を mockall + Sequence で検証する単体テスト。
    //!
    //! 鍵リング backend / SSH agent backend / recipient backend / cipher backend を port mock で差し替え、
    //! envelope 取得→recipient 照合→DEK unwrap→backup 復号→fingerprint 照合→既存鍵衝突→import→subkey
    //! 検証→keygrip 登録→SSH support 充足という順序と、各停止条件を検証する。test double は持ち込まない。

    use crate::features::bws_secrets::ports::public::BwsSecretValue;

    use crate::{
        features::{
            gpg_backup_recovery::domain::{
                commands::RestoreGpgCommand,
                gpg_backup::{BackupUpdateGuard, ConnectedYubiKey, GpgBackupEnvelope},
                gpg_restore::{
                    ImportedKeyComposition, Keygrip, OpenPgpBackupFacts, OpenPgpRevocation,
                    OpenPgpSubkeyFacts, OpenSshPublicKey, ResolvedSubkey, SshAgentReadiness,
                    SubkeyCapability,
                },
            },
            yubikey_lifecycle::ports::public::{SecretManifest, SecretStorageReadInspection},
        },
        foundation::protection::ProtectedSecret,
    };
    mod domain {
        pub(crate) mod bws {
            pub(crate) use crate::features::bws_secrets::ports::public::{
                BwsLookupCandidate, BwsProjectId, BwsSecretId,
            };
        }
        pub(crate) mod gpg_backup {
            pub(crate) use crate::features::gpg_backup_recovery::ports::public::PrimaryFingerprint;
        }
    }
    mod ports {
        pub(crate) use crate::features::bws_secrets::ports::public::MockBwsClientPort;
        pub(crate) use crate::features::cli_interaction::ports::public::MockReportPort;
        pub(crate) use crate::features::gpg_backup_recovery::ports::public::{
            MockBackupCipherPort, MockGpgKeyringPort, MockSshAgentPort,
        };
        pub(crate) use crate::features::yubikey_lifecycle::ports::public::MockGpgRecipientPort;
    }

    use super::{
        RestoreGpgIdentityRuntime, RestoreGpgYubikeyRuntime, combine_main_and_cleanup,
        run_restore_gpg,
    };

    const PRIMARY_FP: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn rollback_failure_keeps_cleanup_detail_out_of_public_display() {
        let error = combine_main_and_cleanup(
            anyhow::anyhow!("main restore failure"),
            Err(anyhow::anyhow!("rollback deletion failure")),
        )
        .expect_err("rollback failure must remain failure");
        assert_eq!(error.to_string(), "GPG restore rollback cleanup failed");
        assert!(!error.to_string().contains("rollback deletion failure"));
        assert!(
            error
                .chain()
                .any(|source| source.to_string() == "main restore failure")
        );
    }
    const KEYGRIP: &str = "AABBCCDDEEFF00112233445566778899AABBCCDD";
    const SSH_PUBLIC_KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITESTBODY restore";

    fn material(bytes: &[u8]) -> crate::Result<ProtectedSecret> {
        ProtectedSecret::from_test_bytes(bytes)
    }

    fn backup_facts() -> crate::Result<OpenPgpBackupFacts> {
        let primary_fingerprint = domain::gpg_backup::PrimaryFingerprint::parse(PRIMARY_FP)?;
        let subkey = |signing, authentication, storage_encryption| OpenPgpSubkeyFacts {
            supported: true,
            alive: true,
            revocation: OpenPgpRevocation::NotAsFarAsWeKnow,
            secret: true,
            signing,
            authentication,
            storage_encryption,
            transport_encryption: false,
        };
        Ok(OpenPgpBackupFacts {
            primary_fingerprint,
            certificate_revocation: OpenPgpRevocation::NotAsFarAsWeKnow,
            subkeys: vec![
                subkey(true, false, false),
                subkey(false, true, false),
                subkey(false, false, true),
            ],
        })
    }

    fn read_inspection() -> crate::Result<SecretStorageReadInspection> {
        Ok(SecretStorageReadInspection {
            manifest_bytes: Some(SecretManifest::fixture_v2().encode()?),
            encoded: Some(vec![1]),
        })
    }

    /// serial 2001 に一致する recipient を 1 件持つ有効 envelope JSON を作る。
    fn envelope() -> crate::Result<GpgBackupEnvelope> {
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
        GpgBackupEnvelope::parse(&json)
    }

    fn envelope_value() -> crate::Result<String> {
        envelope()?.to_json_string()
    }

    fn connected() -> crate::Result<ConnectedYubiKey> {
        ConnectedYubiKey::new(
            "2001",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
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
        storage: &mut crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort,
        sequence: &mut mockall::Sequence,
    ) {
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .in_sequence(sequence)
            .returning(|_, _| read_inspection());
        storage
            .expect_load_secret()
            .times(1)
            .in_sequence(sequence)
            .returning(|_, _| material(b"access-token"));
    }

    #[tokio::test]
    async fn restore_gpg_runs_full_order_and_reports() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut device =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|requested| {
                requested.ok_or_else(|| anyhow::anyhow!("test requires an explicit serial"))
            });
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence);

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().times(1).returning(|_| {
            Ok(vec![domain::bws::BwsLookupCandidate {
                id: domain::bws::BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        bws.expect_list_bws_secrets().times(1).returning(|_, _| {
            Ok(vec![domain::bws::BwsLookupCandidate {
                id: domain::bws::BwsSecretId::new("gpg-id"),
                name: "gpg-secret-key-backup".to_owned(),
            }])
        });
        bws.expect_fetch_gpg_backup_envelope()
            .times(1)
            .returning(|_, _| {
                Ok((
                    BwsSecretValue::from_bytes(envelope_value()?.as_bytes().to_vec()),
                    BackupUpdateGuard::ValueDigest("d".to_owned()),
                ))
            });

        let mut recipient = ports::MockGpgRecipientPort::new();
        recipient
            .expect_resolve_connected_recipient()
            .times(1)
            .returning(|_| connected());
        recipient
            .expect_unwrap_dek()
            .times(1)
            .returning(|_, _| material(b"dek"));

        let mut cipher = ports::MockBackupCipherPort::new();
        cipher
            .expect_decrypt_backup()
            .times(1)
            .returning(|_, _| material(b"backup"));

        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_parse_backup_primary_fingerprint()
            .times(1)
            .returning(|_| backup_facts());
        keyring
            .expect_secret_key_exists()
            .times(1)
            .returning(|_| Ok(false));
        keyring
            .expect_import_secret_key()
            .times(1)
            .returning(|_| domain::gpg_backup::PrimaryFingerprint::parse(PRIMARY_FP));
        keyring
            .expect_inspect_imported_key()
            .times(1)
            .returning(|_| Ok(all_usable_composition()));
        keyring
            .expect_authentication_subkey_keygrip()
            .times(1)
            .returning(|_| Keygrip::parse(KEYGRIP));
        keyring
            .expect_authentication_subkey_ssh_public_key()
            .times(1)
            .returning(|_| OpenSshPublicKey::parse(SSH_PUBLIC_KEY));

        let mut ssh_agent = ports::MockSshAgentPort::new();
        ssh_agent
            .expect_register_authentication_subkey()
            .times(1)
            .returning(|_| Ok(true));
        ssh_agent
            .expect_inspect_ssh_agent()
            .times(1)
            .returning(|_| {
                Ok(SshAgentReadiness {
                    socket_resolved: true,
                    recovery_identity_present: true,
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
            RestoreGpgYubikeyRuntime {
                device: &mut device,
                storage: &mut storage,
            },
            &bws,
            &mut recipient,
            &mut cipher,
            RestoreGpgIdentityRuntime {
                keyring: &mut keyring,
                ssh_agent: &mut ssh_agent,
            },
            &report,
        )
        .await
    }

    #[tokio::test]
    async fn restore_gpg_stops_when_existing_key_collides() {
        let mut device =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_read()
            .returning(|_, _| read_inspection());
        storage
            .expect_load_secret()
            .returning(|_, _| material(b"access-token"));
        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().returning(|_| {
            Ok(vec![domain::bws::BwsLookupCandidate {
                id: domain::bws::BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![domain::bws::BwsLookupCandidate {
                id: domain::bws::BwsSecretId::new("gpg-id"),
                name: "gpg-secret-key-backup".to_owned(),
            }])
        });
        bws.expect_fetch_gpg_backup_envelope().returning(|_, _| {
            Ok((
                BwsSecretValue::from_bytes(envelope_value()?.as_bytes().to_vec()),
                BackupUpdateGuard::ValueDigest("d".to_owned()),
            ))
        });
        let mut recipient = ports::MockGpgRecipientPort::new();
        recipient
            .expect_resolve_connected_recipient()
            .returning(|_| connected());
        recipient
            .expect_unwrap_dek()
            .returning(|_, _| material(b"dek"));
        let mut cipher = ports::MockBackupCipherPort::new();
        cipher
            .expect_decrypt_backup()
            .returning(|_, _| material(b"backup"));
        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_parse_backup_primary_fingerprint()
            .returning(|_| backup_facts());
        keyring.expect_secret_key_exists().returning(|_| Ok(true));
        // 既存鍵衝突で import へ進ませない。
        keyring.expect_import_secret_key().times(0);
        let mut ssh_agent = ports::MockSshAgentPort::new();
        ssh_agent.expect_register_authentication_subkey().times(0);
        let report = ports::MockReportPort::new();

        let result = run_restore_gpg(
            RestoreGpgCommand { serial: Some(2001) },
            RestoreGpgYubikeyRuntime {
                device: &mut device,
                storage: &mut storage,
            },
            &bws,
            &mut recipient,
            &mut cipher,
            RestoreGpgIdentityRuntime {
                keyring: &mut keyring,
                ssh_agent: &mut ssh_agent,
            },
            &report,
        )
        .await;

        assert!(result.is_err(), "existing key collision must stop import");
    }

    /// parser が protected packet として返した failure は gpgme import より前に伝播する。
    ///
    /// actual transferable secret key packet の解析は protection backend の unit test が担い、ここでは
    /// application の caller/停止境界だけを port contract で検証する。
    #[tokio::test]
    async fn restore_gpg_stops_on_protected_packet_parser_failure_before_import()
    -> crate::Result<()> {
        let mut device =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_read()
            .returning(|_, _| read_inspection());
        storage
            .expect_load_secret()
            .returning(|_, _| material(b"access-token"));

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().returning(|_| {
            Ok(vec![domain::bws::BwsLookupCandidate {
                id: domain::bws::BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![domain::bws::BwsLookupCandidate {
                id: domain::bws::BwsSecretId::new("gpg-id"),
                name: "gpg-secret-key-backup".to_owned(),
            }])
        });
        bws.expect_fetch_gpg_backup_envelope().returning(|_, _| {
            Ok((
                BwsSecretValue::from_bytes(envelope_value()?.as_bytes().to_vec()),
                BackupUpdateGuard::ValueDigest("d".to_owned()),
            ))
        });

        let mut recipient = ports::MockGpgRecipientPort::new();
        recipient
            .expect_resolve_connected_recipient()
            .returning(|_| connected());
        recipient
            .expect_unwrap_dek()
            .returning(|_, _| material(b"dek"));
        let mut cipher = ports::MockBackupCipherPort::new();
        cipher
            .expect_decrypt_backup()
            .returning(|_, _| material(b"protected-packet-fixture"));

        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_parse_backup_primary_fingerprint()
            .times(1)
            .returning(|_| {
                Err(anyhow::anyhow!(
                    "OpenPGP backup contains passphrase-protected secret key material"
                ))
            });
        keyring.expect_secret_key_exists().times(0);
        keyring.expect_import_secret_key().times(0);
        keyring.expect_inspect_imported_key().times(0);
        keyring.expect_authentication_subkey_keygrip().times(0);
        keyring
            .expect_authentication_subkey_ssh_public_key()
            .times(0);
        keyring.expect_delete_secret_key().times(0);
        let mut ssh_agent = ports::MockSshAgentPort::new();
        ssh_agent.expect_register_authentication_subkey().times(0);
        ssh_agent.expect_inspect_ssh_agent().times(0);
        let report = ports::MockReportPort::new();

        let error = run_restore_gpg(
            RestoreGpgCommand { serial: Some(2001) },
            RestoreGpgYubikeyRuntime {
                device: &mut device,
                storage: &mut storage,
            },
            &bws,
            &mut recipient,
            &mut cipher,
            RestoreGpgIdentityRuntime {
                keyring: &mut keyring,
                ssh_agent: &mut ssh_agent,
            },
            &report,
        )
        .await
        .expect_err("protected packet must stop before gpgme import");
        assert!(
            error
                .to_string()
                .contains("OpenPGP backup contains passphrase-protected secret key material"),
            "protected backup must reach the encrypted-packet rejection: {error:#}"
        );
        Ok(())
    }

    /// gpgme import result が envelope/Sequoia で確定した primary fingerprint と異なる場合、今回 import
    /// した key だけを一度 rollback し、inspection、SSH agent、success report へ進まない。
    #[tokio::test]
    async fn restore_gpg_rolls_back_once_when_imported_fingerprint_mismatches() {
        let imported_fp = "fedcba9876543210fedcba9876543210fedcba98";
        let mut device =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_read()
            .returning(|_, _| read_inspection());
        storage
            .expect_load_secret()
            .returning(|_, _| material(b"access-token"));
        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().returning(|_| {
            Ok(vec![domain::bws::BwsLookupCandidate {
                id: domain::bws::BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![domain::bws::BwsLookupCandidate {
                id: domain::bws::BwsSecretId::new("gpg-id"),
                name: "gpg-secret-key-backup".to_owned(),
            }])
        });
        bws.expect_fetch_gpg_backup_envelope().returning(|_, _| {
            Ok((
                BwsSecretValue::from_bytes(envelope()?.to_json()?),
                BackupUpdateGuard::ValueDigest("d".to_owned()),
            ))
        });
        let mut recipient = ports::MockGpgRecipientPort::new();
        recipient
            .expect_resolve_connected_recipient()
            .returning(|_| connected());
        recipient
            .expect_unwrap_dek()
            .returning(|_, _| material(b"dek"));
        let mut cipher = ports::MockBackupCipherPort::new();
        cipher
            .expect_decrypt_backup()
            .returning(|_, _| material(b"backup"));
        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_parse_backup_primary_fingerprint()
            .returning(|_| backup_facts());
        keyring.expect_secret_key_exists().returning(|_| Ok(false));
        keyring
            .expect_import_secret_key()
            .times(1)
            .returning(move |_| domain::gpg_backup::PrimaryFingerprint::parse(imported_fp));
        keyring
            .expect_delete_secret_key()
            .times(1)
            .withf(move |fingerprint| fingerprint.as_str() == imported_fp)
            .returning(|_| Ok(()));
        keyring.expect_inspect_imported_key().times(0);
        keyring.expect_authentication_subkey_keygrip().times(0);
        keyring
            .expect_authentication_subkey_ssh_public_key()
            .times(0);
        let mut ssh_agent = ports::MockSshAgentPort::new();
        ssh_agent.expect_register_authentication_subkey().times(0);
        ssh_agent.expect_inspect_ssh_agent().times(0);
        let report = ports::MockReportPort::new();

        let error = run_restore_gpg(
            RestoreGpgCommand { serial: Some(2001) },
            RestoreGpgYubikeyRuntime {
                device: &mut device,
                storage: &mut storage,
            },
            &bws,
            &mut recipient,
            &mut cipher,
            RestoreGpgIdentityRuntime {
                keyring: &mut keyring,
                ssh_agent: &mut ssh_agent,
            },
            &report,
        )
        .await
        .expect_err("mismatching import result must be rolled back");
        assert!(
            error
                .to_string()
                .contains("GPG import primary fingerprint does not match decrypted backup")
        );
    }

    /// import 後の subkey 検証に失敗した場合、不完全鍵を残さないよう `delete_secret_key` を呼んで
    /// ロールバックし、元の検証エラーを返すことを検証する（次回 restore の衝突復旧不能を防ぐ）。
    #[tokio::test]
    async fn restore_gpg_rolls_back_when_verification_fails() {
        let mut device =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_read()
            .returning(|_, _| read_inspection());
        storage
            .expect_load_secret()
            .returning(|_, _| material(b"access-token"));
        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().returning(|_| {
            Ok(vec![domain::bws::BwsLookupCandidate {
                id: domain::bws::BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![domain::bws::BwsLookupCandidate {
                id: domain::bws::BwsSecretId::new("gpg-id"),
                name: "gpg-secret-key-backup".to_owned(),
            }])
        });
        bws.expect_fetch_gpg_backup_envelope().returning(|_, _| {
            Ok((
                BwsSecretValue::from_bytes(envelope_value()?.as_bytes().to_vec()),
                BackupUpdateGuard::ValueDigest("d".to_owned()),
            ))
        });
        let mut recipient = ports::MockGpgRecipientPort::new();
        recipient
            .expect_resolve_connected_recipient()
            .returning(|_| connected());
        recipient
            .expect_unwrap_dek()
            .returning(|_, _| material(b"dek"));
        let mut cipher = ports::MockBackupCipherPort::new();
        cipher
            .expect_decrypt_backup()
            .returning(|_, _| material(b"backup"));

        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_parse_backup_primary_fingerprint()
            .returning(|_| backup_facts());
        keyring.expect_secret_key_exists().returning(|_| Ok(false));
        keyring
            .expect_import_secret_key()
            .times(1)
            .returning(|_| domain::gpg_backup::PrimaryFingerprint::parse(PRIMARY_FP));
        // subkey 検証が不完全（signing subkey 不在）で失敗する構成を返す。
        keyring
            .expect_inspect_imported_key()
            .times(1)
            .returning(|_| {
                Ok(ImportedKeyComposition::new(
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
                    ],
                ))
            });
        // 検証失敗で不完全鍵をロールバック削除する。
        keyring
            .expect_delete_secret_key()
            .times(1)
            .withf(|fingerprint| fingerprint.as_str() == PRIMARY_FP)
            .returning(|_| Ok(()));
        // keygrip 登録・SSH support 確認へは進ませない。
        keyring.expect_authentication_subkey_keygrip().times(0);

        let mut ssh_agent = ports::MockSshAgentPort::new();
        ssh_agent.expect_register_authentication_subkey().times(0);
        ssh_agent.expect_inspect_ssh_agent().times(0);
        let report = ports::MockReportPort::new();

        let result = run_restore_gpg(
            RestoreGpgCommand { serial: Some(2001) },
            RestoreGpgYubikeyRuntime {
                device: &mut device,
                storage: &mut storage,
            },
            &bws,
            &mut recipient,
            &mut cipher,
            RestoreGpgIdentityRuntime {
                keyring: &mut keyring,
                ssh_agent: &mut ssh_agent,
            },
            &report,
        )
        .await;

        assert!(
            result.is_err(),
            "incomplete subkey verification must fail restore after rollback"
        );
    }

    /// import と subkey 検証は成功したが、手順 9-10（keygrip 登録後の SSH support 確認）で停止する場合、
    /// import 済み secret key を `delete_secret_key` で best-effort 削除してから元エラーを返すことを検証する。
    /// これにより設定修正後の再実行が手順 6 の既存鍵衝突で止まらず再 import できる（復元処理の原子化）。
    #[tokio::test]
    async fn restore_gpg_rolls_back_when_ssh_support_fails_after_import() {
        let mut device =
            crate::features::yubikey_lifecycle::ports::public::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut storage =
            crate::features::yubikey_lifecycle::ports::public::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_read()
            .returning(|_, _| read_inspection());
        storage
            .expect_load_secret()
            .returning(|_, _| material(b"access-token"));
        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().returning(|_| {
            Ok(vec![domain::bws::BwsLookupCandidate {
                id: domain::bws::BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![domain::bws::BwsLookupCandidate {
                id: domain::bws::BwsSecretId::new("gpg-id"),
                name: "gpg-secret-key-backup".to_owned(),
            }])
        });
        bws.expect_fetch_gpg_backup_envelope().returning(|_, _| {
            Ok((
                BwsSecretValue::from_bytes(envelope_value()?.as_bytes().to_vec()),
                BackupUpdateGuard::ValueDigest("d".to_owned()),
            ))
        });
        let mut recipient = ports::MockGpgRecipientPort::new();
        recipient
            .expect_resolve_connected_recipient()
            .returning(|_| connected());
        recipient
            .expect_unwrap_dek()
            .returning(|_, _| material(b"dek"));
        let mut cipher = ports::MockBackupCipherPort::new();
        cipher
            .expect_decrypt_backup()
            .returning(|_, _| material(b"backup"));

        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_parse_backup_primary_fingerprint()
            .returning(|_| backup_facts());
        keyring.expect_secret_key_exists().returning(|_| Ok(false));
        keyring
            .expect_import_secret_key()
            .times(1)
            .returning(|_| domain::gpg_backup::PrimaryFingerprint::parse(PRIMARY_FP));
        // subkey 検証は成功する。
        keyring
            .expect_inspect_imported_key()
            .times(1)
            .returning(|_| Ok(all_usable_composition()));
        keyring
            .expect_authentication_subkey_keygrip()
            .times(1)
            .returning(|_| Keygrip::parse(KEYGRIP));
        keyring
            .expect_authentication_subkey_ssh_public_key()
            .times(1)
            .returning(|_| OpenSshPublicKey::parse(SSH_PUBLIC_KEY));
        // import 後の手順 9-10 失敗でも import 済み鍵をロールバック削除する。
        keyring
            .expect_delete_secret_key()
            .times(1)
            .withf(|fingerprint| fingerprint.as_str() == PRIMARY_FP)
            .returning(|_| Ok(()));

        let mut ssh_agent = ports::MockSshAgentPort::new();
        ssh_agent
            .expect_register_authentication_subkey()
            .times(1)
            .returning(|_| Ok(true));
        ssh_agent
            .expect_unregister_authentication_subkey()
            .times(1)
            .withf(|keygrip| keygrip.as_str() == KEYGRIP)
            .returning(|_| Ok(()));
        // SSH support が利用不能（recovery identity を識別できない）で停止する。
        ssh_agent
            .expect_inspect_ssh_agent()
            .times(1)
            .returning(|_| {
                Ok(SshAgentReadiness {
                    socket_resolved: true,
                    recovery_identity_present: false,
                })
            });
        // 停止するため report は書かない。
        let report = ports::MockReportPort::new();

        let result = run_restore_gpg(
            RestoreGpgCommand { serial: Some(2001) },
            RestoreGpgYubikeyRuntime {
                device: &mut device,
                storage: &mut storage,
            },
            &bws,
            &mut recipient,
            &mut cipher,
            RestoreGpgIdentityRuntime {
                keyring: &mut keyring,
                ssh_agent: &mut ssh_agent,
            },
            &report,
        )
        .await;

        assert!(
            result.is_err(),
            "ssh support failure after import must fail restore after rollback"
        );
    }
}
