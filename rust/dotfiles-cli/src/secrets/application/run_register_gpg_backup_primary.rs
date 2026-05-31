//! gpg-secret-key-backup の primary 登録順序を固定し、export/暗号化/登録の実装詳細を port 境界へ閉じる。

use crate::Result;
use crate::secrets::{
    domain::{
        bws::{BwsProjectName, BwsSecretName},
        commands::RegisterGpgBackupCommand,
        gpg_backup::{EnvelopeMetadata, EnvelopeRecipient, GpgBackupEnvelope},
        piv::{SecretName, validate_piv_pin_len},
        storage::SecretStorageReadIntent,
    },
    ports,
};

/// 既存環境の GPG secret key を encrypted envelope 化し、Bitwarden Secrets Manager へ primary 登録する。
///
/// 設計「backup export 入力契約」「recipient 運用 / BWS 更新契約」の primary 登録経路を順序制御として
/// 固定する。export 前に subkey 構成を検証し、export 直後の bytes を再解析して fingerprint 一致を確認した
/// うえで envelope 化し、接続中 YubiKey の recipient を 1 件作って BWS へ登録する。secret key material と
/// DEK は port 境界の保護値として扱い、argv/log/永続ファイルへ出さない。BWS access token は restore 系と
/// 同じく接続中 YubiKey storage から読み出す。順序を application に固定するのは「subkey 検証と fingerprint
/// 一致を満たすまで envelope 化・登録へ進ませない」停止条件の責務境界を保護するためである。既存の同名
/// backup がある場合は重複登録を停止条件とする（recipient 追加・更新は spare 追加 use case が扱う）。
#[expect(
    clippy::too_many_arguments,
    reason = "primary 登録は device/pin/storage/keyring/cipher/recipient/clock/bws の port を順序適用する単一 use case"
)]
pub(crate) async fn run_register_gpg_backup_primary<D, P, S, K, C, Y, T, B>(
    command: RegisterGpgBackupCommand,
    device_serial: &mut D,
    pin_policy: &mut impl ports::DevicePinPolicyPort,
    process: &P,
    storage_port: &mut S,
    keyring: &mut K,
    cipher: &mut C,
    recipient: &mut Y,
    clock: &T,
    bws_client: &B,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    P: ports::PinInputPort,
    S: ports::SecretStoragePort,
    K: ports::GpgKeyringPort,
    C: ports::BackupCipherPort,
    Y: ports::GpgRecipientPort,
    T: ports::ClockPort,
    B: ports::BwsClientPort,
{
    let serial = device_serial.resolve_device_serial(command.serial)?;
    let pin = if pin_policy.device_requires_pin(serial)? {
        let pin = process.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };

    // export 前に encryption / authentication / signing subkey の利用可能状態を検証する。
    keyring
        .inspect_imported_key(&command.primary_fingerprint)?
        .ensure_usable()?;

    // export 直後の bytes を再解析し、導出 fingerprint が指定値と一致する場合だけ envelope 化へ進む。
    let backup = keyring.export_secret_key(&command.primary_fingerprint)?;
    let parsed = keyring.parse_backup_primary_fingerprint(&backup)?;
    if parsed.as_str() != command.primary_fingerprint.as_str() {
        anyhow::bail!("exported gpg backup primary fingerprint does not match the requested key");
    }

    // DEK を生成して backup を暗号化し、接続中 YubiKey の recipient で DEK を wrap する。
    let dek = cipher.generate_dek()?;
    let ciphertext = cipher.encrypt_backup(&dek, &backup)?;
    let recipient_entry: EnvelopeRecipient = recipient.wrap_dek_for_recipient(serial, &dek)?;

    let metadata = EnvelopeMetadata::new(parsed, clock.now_rfc3339_utc()?)?;
    let envelope = GpgBackupEnvelope::assemble(metadata, vec![recipient_entry], ciphertext)?;

    // BWS access token を YubiKey storage から読み出し、復旧 project を解決する。
    let access_token = load_bws_access_token(serial, storage_port, pin.as_ref())?;
    let project_id = BwsProjectName::DOTFILES_SECRET_RECOVERY
        .resolve_id(bws_client.list_bws_projects(&access_token).await?)?;

    // 同名 backup が既にある場合は重複登録を停止条件にする。未登録のときだけ新規作成する。
    let key = BwsSecretName::GpgSecretKeyBackup.key();
    let existing = BwsSecretName::GpgSecretKeyBackup.resolve_id(
        bws_client
            .list_bws_secrets(&access_token, &project_id)
            .await?,
        &project_id,
    );
    if existing.is_ok() {
        anyhow::bail!(
            "a gpg-secret-key-backup secret already exists; refusing to overwrite on primary registration"
        );
    }
    bws_client
        .create_gpg_backup_envelope(&access_token, &project_id, key, &envelope)
        .await
        .map(|_id| ())
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
    //! primary 登録の順序（subkey 検証→export→fingerprint 照合→暗号化→recipient wrap→envelope→BWS 作成）を
    //! mockall + Sequence で検証する単体テスト。
    //!
    //! keyring / cipher / recipient / clock / bws backend を port mock で差し替え、subkey 検証成功と
    //! fingerprint 一致を満たすまで登録へ進ませないこと、未登録のとき create が呼ばれることを確認する。

    use crate::secrets::{
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

    use super::run_register_gpg_backup_primary;

    const PRIMARY_FP: &str = "0123456789abcdef0123456789abcdef01234567";

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn read_inspection() -> SecretStorageReadInspection {
        SecretStorageReadInspection {
            manifest_bytes: Some(SecretManifest::expected().encode().expect("manifest")),
            encoded: Some(vec![1]),
        }
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
            .returning(|_| Ok(false));
        let process = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        storage
            .expect_inspect_secret_storage_read()
            .returning(|_, _| Ok(read_inspection()));
        storage
            .expect_load_secret()
            .returning(|_, _, _| Ok(material(b"access-token")));

        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_inspect_imported_key()
            .times(1)
            .returning(|_| Ok(all_usable()));
        keyring
            .expect_export_secret_key()
            .times(1)
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

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().returning(|_| {
            Ok(vec![crate::secrets::domain::bws::BwsLookupCandidate {
                id: crate::secrets::domain::bws::BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        // 同名 backup は未登録（list は空）。
        bws.expect_list_bws_secrets()
            .returning(|_, _| Ok(Vec::new()));
        bws.expect_create_gpg_backup_envelope()
            .times(1)
            .returning(|_, _, _, _| Ok(crate::secrets::domain::bws::BwsSecretId::new("new-id")));

        run_register_gpg_backup_primary(
            RegisterGpgBackupCommand {
                primary_fingerprint: PrimaryFingerprint::parse(PRIMARY_FP)?,
                serial: Some(2001),
            },
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &mut keyring,
            &mut cipher,
            &mut recipient,
            &clock,
            &bws,
        )
        .await
    }
}
