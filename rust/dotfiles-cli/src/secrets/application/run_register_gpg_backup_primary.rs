//! gpg-secret-key-backup の事前登録状態確認順序を固定し、export/暗号化/登録の実装詳細を port 境界へ閉じる。

use anyhow::Context;

use crate::Result;
use crate::secrets::{
    domain::{
        commands::RegisterGpgBackupCommand,
        gpg_backup::SecretPrimaryKeyCandidates,
        piv::{SecretName, validate_piv_pin_len},
        storage::SecretStorageReadIntent,
        vault::{
            BitwardenAccountApiKey, BitwardenVaultCredentials, VaultLookupResolution,
            VaultSecretName,
        },
    },
    ports,
};

/// `run_register_gpg_backup_primary` が使う外部 capability を named field で束ねる。
pub(crate) struct RegisterGpgBackupPrimaryRuntime<'a, B> {
    pub(crate) device_serial: &'a mut dyn ports::yubikey::DeviceSerialPort,
    pub(crate) pin_policy: &'a mut dyn ports::yubikey::DevicePinPolicyPort,
    pub(crate) pin_input: &'a dyn ports::io::PinInputPort,
    pub(crate) secret_input: &'a dyn ports::io::SecretInputPort,
    pub(crate) storage: &'a mut dyn ports::yubikey::SecretStoragePort,
    pub(crate) keyring: &'a mut dyn ports::gpg::GpgKeyringPort,
    pub(crate) store: &'a dyn ports::git::PasswordStorePort,
    pub(crate) recipient: &'a mut dyn ports::yubikey::GpgRecipientPort,
    pub(crate) vault_client: &'a B,
}

/// 既存 `gpg-secret-key-backup` envelope が、この CLI で使える復旧到達状態かを確認する。
///
/// 現行 CLI で実装済みなのは、個人 vault にある既存 envelope の照合経路だけである。
/// Bitwarden vault の同名 backup 重複確認を secret key export より前に行い、
/// 上書きしないと決まっているシナリオで鍵素材をメモリへ載せず pinentry/touch も発生させない。重複がない
/// 場合でも、現行 CLI 経路では 2 recipient 以上の envelope を作れないため、secret key export と
/// DEK 暗号化へ進む前に停止する。secret key material と DEK は port 境界の保護値として扱い、argv/log/
/// 永続ファイルへ出さない。
///
/// Bitwarden 個人 vault への接続には、YubiKey storage に保存済みの Bitwarden account API key
/// `bitwarden-client-id` / `bitwarden-client-secret` と、CLI/app input port で取得した master password を
/// 使う。CLI は secret・token・fingerprint を argv/stdin/env で受け取らず、既存 envelope の確認では接続中 YubiKey の recipient identity だけを解決する。
/// 新規 envelope 作成は 2 recipient 以上を同時取得できる CLI 経路ができるまで拒否する。
///
/// 順序を application に固定するのは「既存 envelope の primary 一致と 2 recipient 到達状態を満たすまで
/// export・envelope 化・登録へ進ませない」停止条件の責務境界を保護するためである。
/// 既存の同名 backup が 1 件ある場合は metadata の primary fingerprint が解決済み primary fingerprint と
/// 一致し、2 recipient 以上かつ接続中 YubiKey の public key fingerprint recipient が含まれる場合だけ
/// 成功扱いにする。envelope 変更はこの CLI 経路では扱わない。
pub(crate) async fn run_register_gpg_backup_primary<B>(
    command: RegisterGpgBackupCommand,
    runtime: RegisterGpgBackupPrimaryRuntime<'_, B>,
) -> Result<()>
where
    B: ports::bw::VaultClientPort,
{
    let RegisterGpgBackupPrimaryRuntime {
        device_serial,
        pin_policy,
        pin_input,
        secret_input,
        storage,
        keyring,
        store,
        recipient,
        vault_client,
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
    let serial = device_serial.resolve_device_serial()?;
    let pin = if pin_policy.device_requires_pin(serial)? {
        let pin = pin_input.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };
    let credentials = (|| -> Result<BitwardenVaultCredentials> {
        let client_id_storage = SecretName::BitwardenClientId.storage_spec(serial);
        let client_id_inspection =
            storage.inspect_secret_storage_read(serial, &client_id_storage)?;
        let client_id_intent =
            SecretStorageReadIntent::from_inspection(client_id_storage, client_id_inspection)?;
        let client_id = storage
            .load_secret(serial, &client_id_intent, pin.as_ref())
            .map_err(|error| client_id_intent.decode_error(error))?;
        client_id_intent.validate_loaded_secret(&client_id)?;
        let client_secret_storage = SecretName::BitwardenClientSecret.storage_spec(serial);
        let client_secret_inspection =
            storage.inspect_secret_storage_read(serial, &client_secret_storage)?;
        let client_secret_intent = SecretStorageReadIntent::from_inspection(
            client_secret_storage,
            client_secret_inspection,
        )?;
        let client_secret = storage
            .load_secret(serial, &client_secret_intent, pin.as_ref())
            .map_err(|error| client_secret_intent.decode_error(error))?;
        client_secret_intent.validate_loaded_secret(&client_secret)?;
        let master_password = secret_input.read_bitwarden_master_password()?;
        Ok(BitwardenVaultCredentials::new(
            BitwardenAccountApiKey::new(client_id, client_secret),
            master_password,
        ))
    })()
    .context("`gpg-backup register` failed while reading Bitwarden vault credentials")?;

    // 同名 backup の有無を export より前に確認する。既存 1 件で primary が一致すれば設定済み secret を
    // 使用し、secret key export・DEK 暗号化・recipient wrap を再実行しない。0 件は現行 CLI 経路で
    // 2 recipient 以上の envelope を作れないため、secret key material を読む前に停止する。
    let candidates = vault_client
        .list_vault_secrets(&credentials)
        .await
        .with_context(|| {
            format!(
                "`gpg-backup register` failed while listing vault secret `{}`",
                VaultSecretName::GpgSecretKeyBackup.key()
            )
        })?;
    match VaultSecretName::GpgSecretKeyBackup.resolve_lookup(candidates) {
        VaultLookupResolution::Missing => {
            anyhow::bail!(
                "gpg-secret-key-backup is not registered; current CLI cannot create a multi-recipient envelope"
            )
        }
        VaultLookupResolution::Unique(secret_id) => {
            let envelope = vault_client
                .fetch_gpg_backup_envelope(&credentials, &secret_id)
                .await
                .with_context(|| {
                    format!(
                        "`gpg-backup register` failed while loading vault secret `{}`",
                        VaultSecretName::GpgSecretKeyBackup.key()
                    )
                })?;
            if envelope.metadata().primary_fingerprint().as_str() != primary_fingerprint.as_str() {
                anyhow::bail!(
                    "existing gpg-secret-key-backup primary fingerprint does not match the resolved key"
                );
            }
            envelope.ensure_recovery_recipient_count()?;
            let connected = recipient.resolve_connected_recipient(serial)?;
            envelope.resolve_recipient(&connected)?;
            Ok(())
        }
        VaultLookupResolution::Ambiguous => anyhow::bail!(
            "multiple gpg-secret-key-backup secrets exist in the personal vault; refusing to provision"
        ),
    }
}

#[cfg(test)]
mod tests {
    use crate::secrets::{
        domain::{
            commands::RegisterGpgBackupCommand,
            gpg_backup::{
                ConnectedYubiKey, EnvelopeCiphertext, EnvelopeMetadata, EnvelopeRecipient,
                GpgBackupEnvelope, PrimaryFingerprint, SecretPrimaryKeyCandidates,
            },
            manifest::SecretManifest,
            piv::SecretName,
            storage::SecretStorageReadInspection,
            vault::{VaultLookupCandidate, VaultSecretId, VaultSecretName},
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::{RegisterGpgBackupPrimaryRuntime, run_register_gpg_backup_primary};

    const PRIMARY_FINGERPRINT: &str = "1111111111111111111111111111111111111111";
    const CONNECTED_RECIPIENT_FINGERPRINT: &str =
        "2222222222222222222222222222222222222222222222222222222222222222";
    const SPARE_RECIPIENT_FINGERPRINT: &str =
        "3333333333333333333333333333333333333333333333333333333333333333";

    fn material(bytes: &'static [u8]) -> crate::Result<ProtectedSecret> {
        ProtectedSecret::from_test_bytes(bytes)
    }

    fn material_for_name(name: SecretName) -> crate::Result<ProtectedSecret> {
        match name {
            SecretName::BitwardenClientId => material(b"client-id"),
            SecretName::BitwardenClientSecret => material(b"client-secret"),
        }
    }

    fn read_inspection() -> crate::Result<SecretStorageReadInspection> {
        Ok(SecretStorageReadInspection {
            manifest_bytes: Some(SecretManifest::expected().encode()?),
            encoded: Some(vec![1]),
        })
    }

    fn primary_fingerprint() -> crate::Result<PrimaryFingerprint> {
        PrimaryFingerprint::parse(PRIMARY_FINGERPRINT)
    }

    fn connected_recipient(serial: u32) -> crate::Result<ConnectedYubiKey> {
        let _ = serial;
        ConnectedYubiKey::new(CONNECTED_RECIPIENT_FINGERPRINT)
    }

    fn gpg_backup_envelope(serial: u32) -> crate::Result<GpgBackupEnvelope> {
        let connected = connected_recipient(serial)?;
        let spare = ConnectedYubiKey::new(SPARE_RECIPIENT_FINGERPRINT)?;
        GpgBackupEnvelope::assemble(
            EnvelopeMetadata::new(primary_fingerprint()?, "2026-01-01T00:00:00Z")?,
            vec![
                EnvelopeRecipient::new(&connected, vec![1])?,
                EnvelopeRecipient::new(&spare, vec![2])?,
            ],
            EnvelopeCiphertext::new(vec![0; 12], vec![1], vec![0; 16])?,
        )
    }

    fn expect_loaded_yubikey_secret(
        storage: &mut ports::yubikey::MockSecretStoragePort,
        sequence: &mut mockall::Sequence,
        serial: u32,
        name: SecretName,
    ) {
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .withf(move |actual_serial, storage| *actual_serial == serial && storage.name == name)
            .in_sequence(sequence)
            .returning(|_, _| read_inspection());
        storage
            .expect_load_secret()
            .times(1)
            .withf(move |actual_serial, intent, pin| {
                *actual_serial == serial && intent.storage.name == name && pin.is_none()
            })
            .in_sequence(sequence)
            .returning(|_, intent, _| material_for_name(intent.storage.name));
    }

    fn forbid_yubikey_storage_writes(storage: &mut ports::yubikey::MockSecretStoragePort) {
        storage.expect_inspect_secret_storage_setup().times(0);
        storage.expect_initialize_secret_storage().times(0);
        storage.expect_finalize_secret_storage_setup().times(0);
        storage.expect_inspect_secret_storage_write().times(0);
        storage.expect_store_secret().times(0);
    }

    /// vault 照合前に master password を input port から 1 回だけ取得し、YubiKey storage へ保存しない。
    #[tokio::test]
    async fn gpg_backup_register_reads_master_password_after_yubikey_api_key_without_storage_write()
    -> crate::Result<()> {
        let serial = 7002;
        let mut sequence = mockall::Sequence::new();
        let mut store = ports::git::MockPasswordStorePort::new();
        store
            .expect_password_store_exists()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(false));
        store.expect_inspect_password_store().times(0);
        store.expect_configured_origin_remote().times(0);
        let mut keyring = ports::gpg::MockGpgKeyringPort::new();
        keyring
            .expect_list_secret_primary_fingerprints()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| {
                Ok(SecretPrimaryKeyCandidates::new(
                    vec![primary_fingerprint()?],
                ))
            });
        keyring.expect_primary_fingerprint_for_recipient().times(0);
        let mut device_serial = ports::yubikey::MockDeviceSerialPort::new();
        device_serial
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(move || Ok(serial));
        let mut pin_policy = ports::yubikey::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(false));
        let mut pin_input = ports::io::MockPinInputPort::new();
        pin_input.expect_read_pin().times(0);
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        forbid_yubikey_storage_writes(&mut storage);
        expect_loaded_yubikey_secret(
            &mut storage,
            &mut sequence,
            serial,
            SecretName::BitwardenClientId,
        );
        expect_loaded_yubikey_secret(
            &mut storage,
            &mut sequence,
            serial,
            SecretName::BitwardenClientSecret,
        );
        let mut secret_input = ports::io::MockSecretInputPort::new();
        secret_input
            .expect_read_bitwarden_client_id_secret()
            .times(0);
        secret_input.expect_read_bitwarden_client_secret().times(0);
        secret_input
            .expect_read_bitwarden_master_password()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| material(b"master-password"));
        let mut vault_client = ports::bw::MockVaultClientPort::new();
        vault_client
            .expect_list_vault_secrets()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| {
                Ok(vec![VaultLookupCandidate {
                    id: VaultSecretId::new("gpg"),
                    name: VaultSecretName::GpgSecretKeyBackup.key().to_owned(),
                }])
            });
        vault_client
            .expect_fetch_gpg_backup_envelope()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(move |_, _| gpg_backup_envelope(serial));
        vault_client.expect_fetch_password_store_remote().times(0);
        vault_client.expect_create_password_store_remote().times(0);
        let mut recipient = ports::yubikey::MockGpgRecipientPort::new();
        recipient
            .expect_resolve_connected_recipient()
            .times(1)
            .withf(move |actual_serial| *actual_serial == serial)
            .in_sequence(&mut sequence)
            .returning(move |_| connected_recipient(serial));
        recipient.expect_unwrap_dek().times(0);

        run_register_gpg_backup_primary(
            RegisterGpgBackupCommand,
            RegisterGpgBackupPrimaryRuntime {
                device_serial: &mut device_serial,
                pin_policy: &mut pin_policy,
                pin_input: &pin_input,
                secret_input: &secret_input,
                storage: &mut storage,
                keyring: &mut keyring,
                store: &store,
                recipient: &mut recipient,
                vault_client: &vault_client,
            },
        )
        .await?;

        Ok(())
    }
}
