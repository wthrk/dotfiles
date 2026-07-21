//! gpg-secret-key-backup の primary 登録順序を固定し、export/暗号化/登録の実装詳細を port 境界へ閉じる。

use crate::Result;
use crate::{
    domain::{
        bws::{BwsProjectName, BwsSecretName},
        commands::RegisterGpgBackupCommand,
        gpg_backup::{EnvelopeMetadata, EnvelopeRecipient, GpgBackupEnvelope},
        piv::SecretName,
        storage::SecretStorageReadIntent,
    },
    ports,
};

/// `run_register_gpg_backup_primary` が使う外部 capability を named field で束ねる。
pub(crate) struct RegisterGpgBackupPrimaryRuntime<'a, B> {
    pub(crate) device: &'a mut dyn ports::DeviceSerialPort,
    pub(crate) storage: &'a mut dyn ports::SecretStoragePort,
    pub(crate) keyring: &'a mut dyn ports::GpgKeyringPort,
    pub(crate) cipher: &'a mut dyn ports::BackupCipherPort,
    pub(crate) recipient: &'a mut dyn ports::GpgRecipientPort,
    pub(crate) clock: &'a dyn ports::ClockPort,
    pub(crate) bws_client: &'a B,
}

/// 既存環境の GPG secret key を encrypted envelope 化し、Bitwarden Secrets Manager へ primary 登録する。
///
/// 設計「backup export 入力契約」「recipient 運用 / BWS 更新契約」の primary 登録経路を順序制御として
/// 固定する。BWS の同名 backup 重複確認を secret key export より前に行い、上書きしないと決まっている
/// シナリオで鍵素材をメモリへ載せず touch も発生させない。重複がない場合だけ subkey 構成を検証
/// し、export 直後の bytes を再解析して fingerprint 一致を確認したうえで envelope 化し、接続中 YubiKey の
/// recipient を 1 件作って BWS へ登録する。secret key material と DEK は port 境界の保護値として扱い、
/// argv/log/永続ファイルへ出さない。
///
/// BWS への登録には YubiKey storage の `bitwarden-client-secret` を使う。token は hidden prompt / pipe
/// から受け取らず、`--serial` または単一接続で解決した YubiKey から読み出す。YubiKey 本体は recipient
/// wrap（PIV slot `82` 公開鍵で DEK を RSA-OAEP wrap）にも必要なため、同じ device serial を使う。slot `82`
/// 公開鍵での wrap は touch を要さない。
///
/// 順序を application に固定するのは「重複確認・subkey 検証・fingerprint 一致を満たすまで export・
/// envelope 化・登録へ進ませない」停止条件の責務境界を保護するためである。既存の同名 backup がある場合は
/// 重複登録を停止条件とする（recipient 追加・更新は spare 追加 use case が扱う）。
pub(crate) async fn run_register_gpg_backup_primary<B>(
    command: RegisterGpgBackupCommand,
    runtime: RegisterGpgBackupPrimaryRuntime<'_, B>,
) -> Result<()>
where
    B: ports::BwsClientPort,
{
    let RegisterGpgBackupPrimaryRuntime {
        device,
        storage,
        keyring,
        cipher,
        recipient,
        clock,
        bws_client,
    } = runtime;
    let primary_fingerprint = match command.primary_fingerprint {
        Some(fp) => fp,
        None => anyhow::bail!("--primary-fingerprint is required for gpg-backup register"),
    };

    // recipient wrap と BWS token 読み出しに使う YubiKey の serial を解決する。
    let serial = device.resolve_device_serial(command.serial)?;

    // BWS 登録用 access token は YubiKey storage の `bitwarden-client-secret` から取得する。
    let access_token = load_bitwarden_client_secret(serial, storage)?;
    let project_id = BwsProjectName::DOTFILES_SECRET_RECOVERY
        .resolve_id(bws_client.list_bws_projects(&access_token).await?)?;

    // 同名 backup が既にある場合は重複登録を停止条件にする。上書きしないと決まっているシナリオで
    // secret key export・DEK 暗号化・recipient wrap を発生させないよう、export より前に重複確認する。
    // `resolve_id` は 0 件と複数件をどちらも `Err` にするため、既存重複 project を「未登録」と
    // 誤認しないよう、同名候補の件数を数えて 1 件以上で停止する。対象同一性の exact match は domain
    // helper に委ね、application は重複時の停止分岐だけを扱う。
    let existing_count = bws_client
        .list_bws_secrets(&access_token, &project_id)
        .await?
        .into_iter()
        .filter(|candidate| BwsSecretName::GpgSecretKeyBackup.matches_candidate(candidate))
        .count();
    if existing_count >= 1 {
        anyhow::bail!(
            "a gpg-secret-key-backup secret already exists; refusing to overwrite on primary registration"
        );
    }

    // export 前に encryption / authentication / signing subkey の利用可能状態を検証する。
    keyring
        .inspect_imported_key(&primary_fingerprint)?
        .ensure_usable()?;

    // export 直後の bytes を再解析し、導出 fingerprint が指定値と一致する場合だけ envelope 化へ進む。
    let backup = keyring.export_secret_key(&primary_fingerprint)?;
    let parsed = keyring.parse_backup_primary_fingerprint(&backup)?;
    if parsed.as_str() != primary_fingerprint.as_str() {
        anyhow::bail!("exported gpg backup primary fingerprint does not match the requested key");
    }

    // DEK を生成して backup を暗号化し、接続中 YubiKey の recipient で DEK を wrap する。
    let dek = cipher.generate_dek()?;
    let ciphertext = cipher.encrypt_backup(&dek, &backup)?;
    let recipient_entry: EnvelopeRecipient = recipient.wrap_dek_for_recipient(serial, &dek)?;

    let metadata = EnvelopeMetadata::new(parsed, clock.now_rfc3339_utc()?)?;
    let envelope = GpgBackupEnvelope::assemble(metadata, vec![recipient_entry], ciphertext)?;

    bws_client
        .create_gpg_backup_envelope(
            &access_token,
            &project_id,
            BwsSecretName::GpgSecretKeyBackup.key(),
            &envelope,
        )
        .await
        .map(|_id| ())
}

/// bitwarden-client-secret を YubiKey storage の read 経路（inspect → intent → load → validate）で取得する。
fn load_bitwarden_client_secret(
    serial: u32,
    storage_port: &mut dyn ports::SecretStoragePort,
) -> Result<crate::support::protection::ProtectedSecret> {
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
    //! primary 登録の順序（重複確認→subkey 検証→export→fingerprint 照合→暗号化→recipient wrap→
    //! envelope→BWS 作成）を mockall + Sequence で検証する単体テスト。
    //!
    //! YubiKey storage / keyring / cipher / recipient / clock / bws backend を port mock で差し替え、BWS 登録に
    //! 使う access token を `bitwarden-client-secret` 読み出し経路から取得すること、重複確認が export より前に行われること、
    //! subkey 検証成功と fingerprint 一致を満たすまで登録へ進ませないこと、未登録のとき create が呼ばれること、
    //! 重複検出時に export・暗号化・wrap のいずれにも進ませないことを確認する。

    use crate::{
        domain::{
            commands::RegisterGpgBackupCommand,
            gpg_backup::{
                ConnectedYubiKey, EnvelopeCiphertext, EnvelopeRecipient, PrimaryFingerprint,
            },
            gpg_restore::{ImportedKeyComposition, ResolvedSubkey, SubkeyCapability},
            manifest::SecretManifest,
            storage::SecretStorageReadInspection,
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::{RegisterGpgBackupPrimaryRuntime, run_register_gpg_backup_primary};

    const PRIMARY_FP: &str = "0123456789abcdef0123456789abcdef01234567";

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn read_inspection() -> SecretStorageReadInspection {
        SecretStorageReadInspection {
            manifest_bytes: Some(SecretManifest::fixture_v2().encode().expect("manifest")),
            encoded: Some(vec![1]),
        }
    }

    struct YubiKeyAccessMocks {
        device: ports::MockDeviceSerialPort,
        storage: ports::MockSecretStoragePort,
    }

    /// BWS access token を YubiKey storage の `bitwarden-client-secret` から読む port mock を共通設定する。
    fn yubikey_access_mocks() -> YubiKeyAccessMocks {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .returning(|requested| Ok(requested.expect("serial")));
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .returning(|_, _| Ok(read_inspection()));
        storage
            .expect_load_secret()
            .times(1)
            .returning(|_, _| Ok(material(b"access-token")));
        YubiKeyAccessMocks { device, storage }
    }

    fn ciphertext() -> EnvelopeCiphertext {
        EnvelopeCiphertext::new(vec![0u8; 12], b"encrypted".to_vec(), vec![0u8; 16])
            .expect("ciphertext")
    }

    fn recipient_entry() -> EnvelopeRecipient {
        let connected = ConnectedYubiKey::new(
            "2001",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .expect("connected");
        EnvelopeRecipient::new(&connected, b"wrapped".to_vec()).expect("recipient")
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

    #[tokio::test]
    async fn register_primary_creates_envelope_when_absent() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut yk = yubikey_access_mocks();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().returning(|_| {
            Ok(vec![crate::domain::bws::BwsLookupCandidate {
                id: crate::domain::bws::BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        // 同名 backup は未登録（list は空）。重複確認は export より前に行う。
        bws.expect_list_bws_secrets()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(Vec::new()));

        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_inspect_imported_key()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(all_usable()));
        keyring
            .expect_export_secret_key()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(material(b"backup")));
        keyring
            .expect_parse_backup_primary_fingerprint()
            .times(1)
            .returning(|_| PrimaryFingerprint::parse(PRIMARY_FP));

        let mut cipher = ports::MockBackupCipherPort::new();
        cipher
            .expect_generate_dek()
            .times(1)
            .returning(|| Ok(material(b"dek")));
        cipher
            .expect_encrypt_backup()
            .times(1)
            .returning(|_, _| Ok(ciphertext()));

        let mut recipient = ports::MockGpgRecipientPort::new();
        recipient
            .expect_wrap_dek_for_recipient()
            .times(1)
            .returning(|_, _| Ok(recipient_entry()));

        let mut clock = ports::MockClockPort::new();
        clock
            .expect_now_rfc3339_utc()
            .times(1)
            .returning(|| Ok("2026-05-31T00:00:00Z".to_owned()));

        bws.expect_create_gpg_backup_envelope()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|_, _, key, _| key == &"gpg-secret-key-backup")
            .returning(|_, _, _, _| Ok(crate::domain::bws::BwsSecretId::new("new-id")));

        run_register_gpg_backup_primary(
            RegisterGpgBackupCommand {
                primary_fingerprint: Some(PrimaryFingerprint::parse(PRIMARY_FP)?),
                serial: Some(2001),
            },
            RegisterGpgBackupPrimaryRuntime {
                device: &mut yk.device,
                storage: &mut yk.storage,
                keyring: &mut keyring,
                cipher: &mut cipher,
                recipient: &mut recipient,
                clock: &clock,
                bws_client: &bws,
            },
        )
        .await
    }

    /// primary fingerprint 未指定時は必須引数エラーで停止し、鍵素材の export や BWS 作成へ進まない。
    #[tokio::test]
    async fn register_primary_requires_primary_fingerprint_before_backup_work() {
        let mut device = ports::MockDeviceSerialPort::new();
        device.expect_resolve_device_serial().times(0);
        let mut storage = ports::MockSecretStoragePort::new();
        storage.expect_inspect_secret_storage_read().times(0);
        storage.expect_load_secret().times(0);

        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring.expect_inspect_imported_key().times(0);
        keyring.expect_export_secret_key().times(0);
        keyring.expect_parse_backup_primary_fingerprint().times(0);

        let mut cipher = ports::MockBackupCipherPort::new();
        cipher.expect_generate_dek().times(0);
        cipher.expect_encrypt_backup().times(0);

        let mut recipient = ports::MockGpgRecipientPort::new();
        recipient.expect_wrap_dek_for_recipient().times(0);

        let mut clock = ports::MockClockPort::new();
        clock.expect_now_rfc3339_utc().times(0);

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().times(0);
        bws.expect_list_bws_secrets().times(0);
        bws.expect_create_gpg_backup_envelope().times(0);

        let result = run_register_gpg_backup_primary(
            RegisterGpgBackupCommand {
                primary_fingerprint: None,
                serial: Some(2001),
            },
            RegisterGpgBackupPrimaryRuntime {
                device: &mut device,
                storage: &mut storage,
                keyring: &mut keyring,
                cipher: &mut cipher,
                recipient: &mut recipient,
                clock: &clock,
                bws_client: &bws,
            },
        )
        .await;

        let error = result.expect_err("missing primary fingerprint must stop registration");
        assert_eq!(
            error.to_string(),
            "--primary-fingerprint is required for gpg-backup register"
        );
    }

    /// 解決済み project 内に同名 backup secret が複数存在する場合は、`resolve_id` の複数件 `Err` を
    /// 「未登録」と誤認せず、create を呼ばずに重複登録を停止することを検証する。
    #[tokio::test]
    async fn register_primary_stops_when_duplicate_secrets_exist() {
        let mut yk = yubikey_access_mocks();

        // 重複検出で export・暗号化・wrap のいずれにも進ませず、鍵素材を不要にメモリへ載せない。
        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring.expect_inspect_imported_key().times(0);
        keyring.expect_export_secret_key().times(0);
        keyring.expect_parse_backup_primary_fingerprint().times(0);

        let mut cipher = ports::MockBackupCipherPort::new();
        cipher.expect_generate_dek().times(0);
        cipher.expect_encrypt_backup().times(0);

        let mut recipient = ports::MockGpgRecipientPort::new();
        recipient.expect_wrap_dek_for_recipient().times(0);

        let mut clock = ports::MockClockPort::new();
        clock.expect_now_rfc3339_utc().times(0);

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().returning(|_| {
            Ok(vec![crate::domain::bws::BwsLookupCandidate {
                id: crate::domain::bws::BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        // 同名 backup が複数件存在する（resolve_id だと複数件 Err になり誤認しうるケース）。
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![
                crate::domain::bws::BwsLookupCandidate {
                    id: crate::domain::bws::BwsSecretId::new("dup-1"),
                    name: "gpg-secret-key-backup".to_owned(),
                },
                crate::domain::bws::BwsLookupCandidate {
                    id: crate::domain::bws::BwsSecretId::new("dup-2"),
                    name: "gpg-secret-key-backup".to_owned(),
                },
            ])
        });
        // 重複検出で create へ進ませない。
        bws.expect_create_gpg_backup_envelope().times(0);

        let result = run_register_gpg_backup_primary(
            RegisterGpgBackupCommand {
                primary_fingerprint: Some(
                    PrimaryFingerprint::parse(PRIMARY_FP).expect("fingerprint"),
                ),
                serial: Some(2001),
            },
            RegisterGpgBackupPrimaryRuntime {
                device: &mut yk.device,
                storage: &mut yk.storage,
                keyring: &mut keyring,
                cipher: &mut cipher,
                recipient: &mut recipient,
                clock: &clock,
                bws_client: &bws,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "duplicate gpg-secret-key-backup secrets must stop primary registration"
        );
    }
}
