//! gpg-secret-key-backup の事前登録状態確認順序を固定し、export/暗号化/登録の実装詳細を port 境界へ閉じる。

use anyhow::Context;

use crate::Result;
use crate::secrets::{
    domain::{
        bws::{BwsLookupResolution, BwsProjectName, BwsSecretName},
        commands::RegisterGpgBackupCommand,
        gpg_backup::SecretPrimaryKeyCandidates,
    },
    ports,
};

/// `run_register_gpg_backup_primary` が使う外部 capability を named field で束ねる。
pub(crate) struct RegisterGpgBackupPrimaryRuntime<'a, B> {
    pub(crate) token_input: &'a dyn ports::BwsAccessTokenInputPort,
    pub(crate) device_serial: &'a mut dyn ports::DeviceSerialPort,
    pub(crate) keyring: &'a mut dyn ports::GpgKeyringPort,
    pub(crate) store: &'a dyn ports::PasswordStorePort,
    pub(crate) recipient: &'a mut dyn ports::GpgRecipientPort,
    pub(crate) bws_client: &'a B,
}

/// 既存 `gpg-secret-key-backup` envelope が、この CLI で使える復旧到達状態かを確認する。
///
/// 設計「backup export 入力契約」「recipient 運用 / BWS 登録契約」のうち、現行 CLI で実装済みなのは
/// 既存 envelope の照合経路だけである。BWS の同名 backup 重複確認を secret key export より前に行い、
/// 上書きしないと決まっているシナリオで鍵素材をメモリへ載せず pinentry/touch も発生させない。重複がない
/// 場合でも、現行 CLI 経路では primary/spare 2 recipient envelope を作れないため、secret key export と
/// DEK 暗号化へ進む前に停止する。secret key material と DEK は port 境界の保護値として扱い、argv/log/
/// 永続ファイルへ出さない。
///
/// BWS への登録には BWS access token を使う。この登録用 token は hidden prompt / pipe から
/// `BwsAccessTokenInputPort` 経由で取得し、YubiKey へ保存しない。YubiKey へ保存する `bws-access-token` は
/// 復旧時の read 用最小権限 token を別経路で用意する。この provisioning command 自体は storage/pin 経由の
/// token 読み出しを行わない。既存 envelope の確認では接続中 YubiKey の recipient identity だけを解決し、
/// 新規 envelope 作成は primary/spare 2 recipient を同時取得できる CLI 経路ができるまで拒否する。
///
/// 順序を application に固定するのは「既存 envelope の primary 一致と 2 recipient 到達状態を満たすまで
/// export・envelope 化・登録へ進ませない」停止条件の責務境界を保護するためである。
/// 既存の同名 backup が 1 件ある場合は metadata の primary fingerprint が解決済み primary fingerprint と
/// 一致し、接続中 YubiKey の recipient が含まれる場合だけ成功扱いにする。envelope 変更はこの CLI 経路では
/// 扱わない。
pub(crate) async fn run_register_gpg_backup_primary<B>(
    command: RegisterGpgBackupCommand,
    runtime: RegisterGpgBackupPrimaryRuntime<'_, B>,
) -> Result<()>
where
    B: ports::BwsClientPort,
{
    let RegisterGpgBackupPrimaryRuntime {
        token_input,
        device_serial,
        keyring,
        store,
        recipient,
        bws_client,
    } = runtime;
    let _ = command;
    let primary_fingerprint = if store.password_store_exists()? {
        let readiness = store.inspect_password_store()?;
        if readiness.gpg_id_present && !readiness.gpg_id_recipients.is_empty() {
            let mut fingerprints = Vec::new();
            for recipient_id in readiness.parse_recipients()? {
                let Some(fingerprint) = keyring.primary_fingerprint_for_recipient(&recipient_id)?
                else {
                    anyhow::bail!(
                        "password-store recipient does not resolve to an available GPG secret key"
                    );
                };
                fingerprints.push(fingerprint);
            }
            SecretPrimaryKeyCandidates::new(fingerprints).resolve_unique()?
        } else {
            keyring
                .list_secret_primary_fingerprints()?
                .resolve_unique()?
        }
    } else {
        keyring
            .list_secret_primary_fingerprints()?
            .resolve_unique()?
    };
    // BWS 登録用 access token を hidden prompt / pipe から取得し、復旧 project を解決する。
    // provisioning command は YubiKey storage を読まず、YubiKey 保存用の復旧 token とは分離する。
    let access_token = token_input
        .read_bws_access_token_for_provisioning()
        .context("`gpg-backup register` failed while reading `bws-access-token (create/use)`")?;
    let project_name = BwsProjectName::DOTFILES_SECRET_RECOVERY;
    let project_candidates = bws_client
        .list_bws_projects(&access_token)
        .await
        .with_context(|| {
            format!(
                "`gpg-backup register` failed while resolving BWS project `{}`",
                project_name.as_str()
            )
        })?;
    let project_id = match project_name.resolve_lookup(project_candidates) {
        BwsLookupResolution::Missing => bws_client
            .create_bws_project(&access_token, project_name)
            .await
            .with_context(|| {
                format!(
                    "`gpg-backup register` failed while creating BWS project `{}`",
                    project_name.as_str()
                )
            })?,
        BwsLookupResolution::Unique(project_id) => project_id,
        BwsLookupResolution::Ambiguous => {
            anyhow::bail!("multiple bws projects matched: {}", project_name.as_str())
        }
    };

    // 同名 backup の有無を export より前に確認する。既存 1 件で primary が一致すれば設定済み secret を
    // 使用し、secret key export・DEK 暗号化・recipient wrap を再実行しない。0 件は現行 CLI 経路で
    // primary/spare 2 recipient envelope を作れないため、secret key material を読む前に停止する。
    let candidates = bws_client
        .list_bws_secrets(&access_token, &project_id)
        .await
        .with_context(|| {
            format!(
                "`gpg-backup register` failed while listing secret `{}` in project `{}`",
                BwsSecretName::GpgSecretKeyBackup.key(),
                project_id.as_str()
            )
        })?;
    match BwsSecretName::GpgSecretKeyBackup.resolve_lookup(candidates) {
        BwsLookupResolution::Missing => {
            anyhow::bail!(
                "gpg-secret-key-backup is not registered; current CLI cannot create a primary/spare recipient envelope"
            )
        }
        BwsLookupResolution::Unique(secret_id) => {
            let envelope = bws_client
                .fetch_gpg_backup_envelope(&access_token, &secret_id)
                .await
                .with_context(|| {
                    format!(
                        "`gpg-backup register` failed while loading secret `{}`",
                        BwsSecretName::GpgSecretKeyBackup.key()
                    )
                })?;
            if envelope.metadata().primary_fingerprint().as_str() != primary_fingerprint.as_str() {
                anyhow::bail!(
                    "existing gpg-secret-key-backup primary fingerprint does not match the resolved key"
                );
            }
            envelope.ensure_spare_recipient_registered()?;
            let serial = device_serial.resolve_device_serial()?;
            let connected = recipient.resolve_connected_recipient(serial)?;
            envelope.resolve_recipient(&connected)?;
            return Ok(());
        }
        BwsLookupResolution::Ambiguous => anyhow::bail!(
            "multiple gpg-secret-key-backup secrets exist in the recovery project; refusing to provision"
        ),
    }
}

#[cfg(test)]
mod tests {
    //! primary 登録の順序（BWS 重複確認→既存 envelope 検証）を mockall + Sequence で検証する単体テスト。
    //!
    //! token-input / keyring / recipient / bws backend を port mock で差し替え、BWS 登録に
    //! 使う access token を BWS access token 入力経路から取得すること、重複確認が export より前に行われること、
    //! 未登録時は create/export/encrypt/wrap/BWS 作成へ進まず secret export 前に停止すること、既存 envelope
    //! の primary fingerprint と 2 recipient 条件、接続中 recipient 一致を満たすまで登録へ
    //! 進ませないことを確認する。

    use crate::secrets::{
        domain::{
            commands::RegisterGpgBackupCommand,
            gpg_backup::{
                ConnectedYubiKey, EnvelopeCiphertext, EnvelopeMetadata, EnvelopeRecipient,
                GpgBackupEnvelope, PrimaryFingerprint, SecretPrimaryKeyCandidates,
            },
            gpg_restore::{ImportedKeyComposition, ResolvedSubkey, SubkeyCapability},
            pass_restore::PasswordStoreReadiness,
        },
        ports,
        ports::ProtectedSecret,
    };

    use super::{RegisterGpgBackupPrimaryRuntime, run_register_gpg_backup_primary};

    const PRIMARY_FP: &str = "0123456789abcdef0123456789abcdef01234567";

    /// `Result::Ok` を取り出す。テストでも `unwrap` / `expect` を使わないための最小 helper。
    fn ok<T, E: std::fmt::Display>(result: std::result::Result<T, E>, context: &str) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("expected {context} to succeed: {error}"),
        }
    }

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ok(ProtectedSecret::from_test_bytes(bytes), "test secret")
    }

    /// BWS access token を hidden prompt / pipe から取得する port mock を共通設定する。
    ///
    /// この mock は hidden prompt / pipe 相当の入力経路として BWS access token を返す。
    /// pin/storage port は構成へ一切渡さず、provisioning command が YubiKey storage を読まないことを固定する。
    fn token_input() -> ports::MockBwsAccessTokenInputPort {
        let mut token_input = ports::MockBwsAccessTokenInputPort::new();
        token_input
            .expect_read_bws_access_token_for_provisioning()
            .times(1)
            .returning(|| Ok(material(b"provisioning-token")));
        token_input
    }

    fn ciphertext() -> EnvelopeCiphertext {
        ok(
            EnvelopeCiphertext::new(vec![0u8; 12], b"encrypted".to_vec(), vec![0u8; 16]),
            "ciphertext",
        )
    }

    fn recipient_entry() -> EnvelopeRecipient {
        let connected = ConnectedYubiKey::new(
            "2001",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        );
        let connected = ok(connected, "connected");
        ok(
            EnvelopeRecipient::new(&connected, b"wrapped".to_vec()),
            "recipient",
        )
    }

    fn existing_envelope(primary_fingerprint: &str) -> GpgBackupEnvelope {
        let spare = ConnectedYubiKey::new(
            "2002",
            "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210",
        );
        let spare = ok(spare, "spare connected");
        ok(
            GpgBackupEnvelope::assemble(
                ok(
                    EnvelopeMetadata::new(
                        ok(
                            PrimaryFingerprint::parse(primary_fingerprint),
                            "fingerprint",
                        ),
                        "2026-05-31T00:00:00Z".to_owned(),
                    ),
                    "metadata",
                ),
                vec![
                    recipient_entry(),
                    ok(
                        EnvelopeRecipient::new(&spare, b"wrapped-spare".to_vec()),
                        "spare recipient",
                    ),
                ],
                ciphertext(),
            ),
            "envelope",
        )
    }

    fn all_usable() -> ImportedKeyComposition {
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

    fn expect_single_primary_resolution(
        keyring: &mut ports::MockGpgKeyringPort,
        store: &mut ports::MockPasswordStorePort,
    ) {
        store
            .expect_password_store_exists()
            .times(1)
            .returning(|| Ok(false));
        keyring
            .expect_list_secret_primary_fingerprints()
            .times(1)
            .returning(|| {
                Ok(SecretPrimaryKeyCandidates::new(vec![
                    PrimaryFingerprint::parse(PRIMARY_FP)?,
                ]))
            });
    }

    #[tokio::test]
    async fn register_primary_stops_before_create_when_new_envelope_has_only_connected_recipient()
    -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let token = token_input();
        let mut device = ports::MockDeviceSerialPort::new();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().returning(|_| {
            Ok(vec![crate::secrets::domain::bws::BwsLookupCandidate {
                id: crate::secrets::domain::bws::BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        // 同名 backup は未登録（list は空）。重複確認は export より前に行う。
        bws.expect_list_bws_secrets()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(Vec::new()));

        device
            .expect_resolve_device_serial()
            .times(0)
            .returning(|| Ok(2001));

        let mut keyring = ports::MockGpgKeyringPort::new();
        let mut store = ports::MockPasswordStorePort::new();
        expect_single_primary_resolution(&mut keyring, &mut store);
        keyring
            .expect_inspect_imported_key()
            .times(0)
            .returning(|_| Ok(all_usable()));
        keyring
            .expect_parse_backup_primary_fingerprint()
            .times(0)
            .returning(|_| PrimaryFingerprint::parse(PRIMARY_FP));

        let mut recipient = ports::MockGpgRecipientPort::new();

        let result = run_register_gpg_backup_primary(
            RegisterGpgBackupCommand,
            RegisterGpgBackupPrimaryRuntime {
                token_input: &token,
                device_serial: &mut device,
                keyring: &mut keyring,
                store: &store,
                recipient: &mut recipient,
                bws_client: &bws,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "new gpg-secret-key-backup must not be persisted with only one recipient"
        );
        Ok(())
    }

    /// 復旧 project 未作成時は project を作成してから、同名 backup の未登録確認後に 1 recipient 作成を拒否する。
    #[tokio::test]
    async fn register_primary_creates_project_when_missing_before_envelope_create()
    -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let token = token_input();
        let mut device = ports::MockDeviceSerialPort::new();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(Vec::new()));
        bws.expect_create_bws_project()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|_, project_name| project_name.as_str() == "dotfiles-secret-recovery")
            .returning(|_, _| {
                Ok(crate::secrets::domain::bws::BwsProjectId::new(
                    "project-new",
                ))
            });
        bws.expect_list_bws_secrets()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, project_id| {
                assert_eq!(project_id.as_str(), "project-new");
                Ok(Vec::new())
            });

        device
            .expect_resolve_device_serial()
            .times(0)
            .returning(|| Ok(2001));

        let mut keyring = ports::MockGpgKeyringPort::new();
        let mut store = ports::MockPasswordStorePort::new();
        expect_single_primary_resolution(&mut keyring, &mut store);
        keyring
            .expect_inspect_imported_key()
            .times(0)
            .returning(|_| Ok(all_usable()));
        keyring
            .expect_parse_backup_primary_fingerprint()
            .times(0)
            .returning(|_| PrimaryFingerprint::parse(PRIMARY_FP));

        let mut recipient = ports::MockGpgRecipientPort::new();

        let result = run_register_gpg_backup_primary(
            RegisterGpgBackupCommand,
            RegisterGpgBackupPrimaryRuntime {
                token_input: &token,
                device_serial: &mut device,
                keyring: &mut keyring,
                store: &store,
                recipient: &mut recipient,
                bws_client: &bws,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "fresh project path must still reject a one-recipient backup envelope"
        );
        Ok(())
    }

    /// 復旧 project が複数一致する場合は、secret 確認・device 解決・鍵 export へ進ませず停止する。
    #[tokio::test]
    async fn register_primary_stops_when_duplicate_projects_exist() {
        let token = token_input();
        let mut device = ports::MockDeviceSerialPort::new();
        device.expect_resolve_device_serial().times(0);

        let mut keyring = ports::MockGpgKeyringPort::new();
        let mut store = ports::MockPasswordStorePort::new();
        expect_single_primary_resolution(&mut keyring, &mut store);
        keyring.expect_inspect_imported_key().times(0);
        keyring.expect_parse_backup_primary_fingerprint().times(0);

        let mut recipient = ports::MockGpgRecipientPort::new();
        recipient.expect_resolve_connected_recipient().times(0);

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().times(1).returning(|_| {
            Ok(vec![
                crate::secrets::domain::bws::BwsLookupCandidate {
                    id: crate::secrets::domain::bws::BwsProjectId::new("project-1"),
                    name: "dotfiles-secret-recovery".to_owned(),
                },
                crate::secrets::domain::bws::BwsLookupCandidate {
                    id: crate::secrets::domain::bws::BwsProjectId::new("project-2"),
                    name: "dotfiles-secret-recovery".to_owned(),
                },
            ])
        });
        bws.expect_create_bws_project().times(0);
        bws.expect_list_bws_secrets().times(0);
        bws.expect_fetch_gpg_backup_envelope().times(0);

        let result = run_register_gpg_backup_primary(
            RegisterGpgBackupCommand,
            RegisterGpgBackupPrimaryRuntime {
                token_input: &token,
                device_serial: &mut device,
                keyring: &mut keyring,
                store: &store,
                recipient: &mut recipient,
                bws_client: &bws,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "duplicate recovery projects must stop gpg backup registration"
        );
    }

    /// 同名 backup が 1 件存在し primary fingerprint と接続中 recipient が一致する場合は、設定済み secret を使い、
    /// export・暗号化・wrap・create へ進まない。
    #[tokio::test]
    async fn register_primary_uses_existing_matching_secret() -> crate::Result<()> {
        let token = token_input();
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|| Ok(2001));

        let mut keyring = ports::MockGpgKeyringPort::new();
        let mut store = ports::MockPasswordStorePort::new();
        expect_single_primary_resolution(&mut keyring, &mut store);
        keyring.expect_inspect_imported_key().times(0);
        keyring.expect_parse_backup_primary_fingerprint().times(0);

        let mut recipient = ports::MockGpgRecipientPort::new();
        recipient
            .expect_resolve_connected_recipient()
            .times(1)
            .withf(|serial| *serial == 2001)
            .returning(|_| {
                ConnectedYubiKey::new(
                    "2001",
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                )
            });

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().returning(|_| {
            Ok(vec![crate::secrets::domain::bws::BwsLookupCandidate {
                id: crate::secrets::domain::bws::BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![crate::secrets::domain::bws::BwsLookupCandidate {
                id: crate::secrets::domain::bws::BwsSecretId::new("backup-id"),
                name: "gpg-secret-key-backup".to_owned(),
            }])
        });
        bws.expect_fetch_gpg_backup_envelope()
            .times(1)
            .returning(|_, _| Ok(existing_envelope(PRIMARY_FP)));

        run_register_gpg_backup_primary(
            RegisterGpgBackupCommand,
            RegisterGpgBackupPrimaryRuntime {
                token_input: &token,
                device_serial: &mut device,
                keyring: &mut keyring,
                store: &store,
                recipient: &mut recipient,
                bws_client: &bws,
            },
        )
        .await
    }

    /// 同名 backup が複数存在する場合は、`resolve_id` の複数件 `Err` を
    /// 「未登録」と誤認せず、create を呼ばずに重複登録を停止することを検証する。
    #[tokio::test]
    async fn register_primary_stops_when_duplicate_secrets_exist() {
        let token = token_input();
        let mut device = ports::MockDeviceSerialPort::new();
        device.expect_resolve_device_serial().times(0);

        // 重複検出で export・暗号化・wrap のいずれにも進ませず、鍵素材を不要にメモリへ載せない。
        let mut keyring = ports::MockGpgKeyringPort::new();
        let mut store = ports::MockPasswordStorePort::new();
        expect_single_primary_resolution(&mut keyring, &mut store);
        keyring.expect_inspect_imported_key().times(0);
        keyring.expect_parse_backup_primary_fingerprint().times(0);

        let mut recipient = ports::MockGpgRecipientPort::new();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().returning(|_| {
            Ok(vec![crate::secrets::domain::bws::BwsLookupCandidate {
                id: crate::secrets::domain::bws::BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        // 同名 backup が複数件存在する（resolve_id だと複数件 Err になり誤認しうるケース）。
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![
                crate::secrets::domain::bws::BwsLookupCandidate {
                    id: crate::secrets::domain::bws::BwsSecretId::new("dup-1"),
                    name: "gpg-secret-key-backup".to_owned(),
                },
                crate::secrets::domain::bws::BwsLookupCandidate {
                    id: crate::secrets::domain::bws::BwsSecretId::new("dup-2"),
                    name: "gpg-secret-key-backup".to_owned(),
                },
            ])
        });
        // 重複検出で create へ進ませない。

        let result = run_register_gpg_backup_primary(
            RegisterGpgBackupCommand,
            RegisterGpgBackupPrimaryRuntime {
                token_input: &token,
                device_serial: &mut device,
                keyring: &mut keyring,
                store: &store,
                recipient: &mut recipient,
                bws_client: &bws,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "duplicate gpg-secret-key-backup secrets must stop primary registration"
        );
    }

    /// fingerprint 未指定かつ store 未初期化時は、鍵リング内の単一 secret primary だけを backup 対象にする。
    #[tokio::test]
    async fn register_primary_without_fingerprint_resolves_single_secret_key() -> crate::Result<()>
    {
        let mut sequence = mockall::Sequence::new();
        let token = token_input();
        let mut device = ports::MockDeviceSerialPort::new();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().returning(|_| {
            Ok(vec![crate::secrets::domain::bws::BwsLookupCandidate {
                id: crate::secrets::domain::bws::BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        bws.expect_list_bws_secrets()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(Vec::new()));

        device
            .expect_resolve_device_serial()
            .times(0)
            .returning(|| Ok(2001));

        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_list_secret_primary_fingerprints()
            .times(1)
            .returning(|| {
                Ok(
                    crate::secrets::domain::gpg_backup::SecretPrimaryKeyCandidates::new(vec![
                        PrimaryFingerprint::parse(PRIMARY_FP)?,
                    ]),
                )
            });
        keyring
            .expect_inspect_imported_key()
            .times(0)
            .returning(|fingerprint| {
                assert_eq!(fingerprint.as_str(), PRIMARY_FP);
                Ok(all_usable())
            });
        keyring
            .expect_parse_backup_primary_fingerprint()
            .times(0)
            .returning(|_| PrimaryFingerprint::parse(PRIMARY_FP));
        let mut store = ports::MockPasswordStorePort::new();
        store
            .expect_password_store_exists()
            .times(1)
            .returning(|| Ok(false));

        let mut recipient = ports::MockGpgRecipientPort::new();

        let result = run_register_gpg_backup_primary(
            RegisterGpgBackupCommand,
            RegisterGpgBackupPrimaryRuntime {
                token_input: &token,
                device_serial: &mut device,
                keyring: &mut keyring,
                store: &store,
                recipient: &mut recipient,
                bws_client: &bws,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "auto-resolved new backup must stop before persisting a one-recipient envelope"
        );
        Ok(())
    }

    /// fingerprint 未指定かつ `.gpg-id` がある場合は、設定済み recipient から解決した primary を使う。
    #[tokio::test]
    async fn register_primary_without_fingerprint_prefers_configured_gpg_id_recipient()
    -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let token = token_input();
        let mut device = ports::MockDeviceSerialPort::new();

        let mut store = ports::MockPasswordStorePort::new();
        store
            .expect_password_store_exists()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(true));
        store
            .expect_inspect_password_store()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| {
                Ok(PasswordStoreReadiness {
                    gpg_id_present: true,
                    gpg_id_recipients: vec!["configured-recipient@example.invalid".to_owned()],
                    sample_entry: None,
                })
            });

        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring.expect_list_secret_primary_fingerprints().times(0);
        keyring
            .expect_primary_fingerprint_for_recipient()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|recipient| {
                assert_eq!(recipient.as_str(), "configured-recipient@example.invalid");
                PrimaryFingerprint::parse(PRIMARY_FP).map(Some)
            });

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().returning(|_| {
            Ok(vec![crate::secrets::domain::bws::BwsLookupCandidate {
                id: crate::secrets::domain::bws::BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        bws.expect_list_bws_secrets()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(Vec::new()));

        device
            .expect_resolve_device_serial()
            .times(0)
            .returning(|| Ok(2001));

        keyring
            .expect_inspect_imported_key()
            .times(0)
            .returning(|fingerprint| {
                assert_eq!(fingerprint.as_str(), PRIMARY_FP);
                Ok(all_usable())
            });
        keyring
            .expect_parse_backup_primary_fingerprint()
            .times(0)
            .returning(|_| PrimaryFingerprint::parse(PRIMARY_FP));

        let mut recipient = ports::MockGpgRecipientPort::new();

        let result = run_register_gpg_backup_primary(
            RegisterGpgBackupCommand,
            RegisterGpgBackupPrimaryRuntime {
                token_input: &token,
                device_serial: &mut device,
                keyring: &mut keyring,
                store: &store,
                recipient: &mut recipient,
                bws_client: &bws,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "configured .gpg-id path must not persist a one-recipient backup envelope"
        );
        Ok(())
    }
}
