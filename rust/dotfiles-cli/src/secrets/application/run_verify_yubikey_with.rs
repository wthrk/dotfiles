//! verify-yubikey の device 解決順序を固定し、外部検証の責務境界を application に維持する。

use crate::Result;
use crate::secrets::{
    domain::{
        commands::VerifyYubikeyCommand,
        piv::{SecretName, validate_piv_pin_len},
        storage::{SecretStorageReadIntent, SecretStorageVerificationPlan},
        vault::{BitwardenAccountApiKey, BitwardenVaultCredentials, VaultSecretName},
        verification::{CheckName, CheckStatus, VerifySummary},
    },
    ports,
};

/// `run_verify_yubikey_with` が使う外部 capability を named field で束ねる。
pub(crate) struct VerifyYubikeyRuntime<'a, B> {
    pub(crate) device: &'a mut dyn ports::yubikey::YubiKeyDevicePort,
    pub(crate) process: &'a dyn ports::io::PinInputPort,
    pub(crate) secret_input: &'a dyn ports::io::SecretInputPort,
    pub(crate) storage: &'a mut dyn ports::yubikey::SecretStoragePort,
    pub(crate) report: &'a dyn ports::io::ReportPort,
    pub(crate) vault_client: &'a B,
    pub(crate) gpg_recipient: &'a mut dyn ports::yubikey::GpgRecipientPort,
}

/// 保存済み secret の存在と、要求された外部確認項目を検証する。
///
/// local storage 検証を外部 vault 確認より前に固定し、bootstrap secret が読めない状態で master
/// password を求めない。master password は vault check が要求された場合だけ `SecretInputPort` から取得し、
/// YubiKey storage には保存せず vault adapter 境界へ渡す。
pub(crate) async fn run_verify_yubikey_with<B>(
    command: VerifyYubikeyCommand,
    runtime: VerifyYubikeyRuntime<'_, B>,
) -> Result<()>
where
    B: ports::bw::VaultClientPort,
{
    let VerifyYubikeyRuntime {
        device,
        process,
        secret_input,
        storage: storage_port,
        report,
        vault_client,
        gpg_recipient,
    } = runtime;
    let requested = command.requested_external_checks()?;
    let serial = device.resolve_device_serial()?;
    let pin = if device.device_requires_pin(serial)? {
        let pin = process.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };

    let local_verify = (|| {
        for storage in SecretStorageVerificationPlan::for_serial(serial).into_targets() {
            let inspection = storage_port.inspect_secret_storage_read(serial, &storage)?;
            let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
            let secret = storage_port
                .load_secret(serial, &intent, pin.as_ref())
                .map_err(|error| intent.decode_error(error))?;
            intent.validate_loaded_secret(&secret)?;
        }
        Ok(())
    })();
    if let Err(err) = local_verify {
        return report
            .write_verify_report(&VerifySummary::local_storage_failed())
            .and(Err(err));
    }
    if requested.is_empty() {
        return report.write_verify_report(&VerifySummary::local_storage_verified());
    }

    let mut summary = VerifySummary::local_storage_verified();
    let mut first_error = None;
    for check in requested {
        match check {
            CheckName::Vault => {
                let credentials = match (|| -> Result<BitwardenVaultCredentials> {
                    let client_id_storage = SecretName::BitwardenClientId.storage_spec(serial);
                    let client_id_inspection =
                        storage_port.inspect_secret_storage_read(serial, &client_id_storage)?;
                    let client_id_intent = SecretStorageReadIntent::from_inspection(
                        client_id_storage,
                        client_id_inspection,
                    )?;
                    let client_id = storage_port
                        .load_secret(serial, &client_id_intent, pin.as_ref())
                        .map_err(|error| client_id_intent.decode_error(error))?;
                    client_id_intent.validate_loaded_secret(&client_id)?;
                    let client_secret_storage =
                        SecretName::BitwardenClientSecret.storage_spec(serial);
                    let client_secret_inspection =
                        storage_port.inspect_secret_storage_read(serial, &client_secret_storage)?;
                    let client_secret_intent = SecretStorageReadIntent::from_inspection(
                        client_secret_storage,
                        client_secret_inspection,
                    )?;
                    let client_secret = storage_port
                        .load_secret(serial, &client_secret_intent, pin.as_ref())
                        .map_err(|error| client_secret_intent.decode_error(error))?;
                    client_secret_intent.validate_loaded_secret(&client_secret)?;
                    let master_password = secret_input.read_bitwarden_master_password()?;
                    Ok(BitwardenVaultCredentials::new(
                        BitwardenAccountApiKey::new(client_id, client_secret),
                        master_password,
                    ))
                })() {
                    Ok(credentials) => credentials,
                    Err(error) => {
                        summary.mark_external_check(CheckName::Vault, CheckStatus::Failed);
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                        continue;
                    }
                };
                match async {
                    let secret_candidates = vault_client.list_vault_secrets(&credentials).await?;

                    let gpg_secret_id = VaultSecretName::GpgSecretKeyBackup
                        .resolve_id(secret_candidates.clone())?;
                    let pass_secret_id =
                        VaultSecretName::PasswordStoreRemote.resolve_id(secret_candidates)?;

                    let envelope = vault_client
                        .fetch_gpg_backup_envelope(&credentials, &gpg_secret_id)
                        .await?;
                    envelope.ensure_recovery_recipient_count()?;
                    let connected = gpg_recipient.resolve_connected_recipient(serial)?;
                    envelope.resolve_recipient(&connected)?;

                    vault_client
                        .fetch_password_store_remote(&credentials, &pass_secret_id)
                        .await
                        .map(|_| ())
                }
                .await
                {
                    Ok(_) => summary.mark_external_check(CheckName::Vault, CheckStatus::Ok),
                    Err(error) => {
                        summary.mark_external_check(CheckName::Vault, CheckStatus::Failed);
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                    }
                }
            }
            CheckName::Setup
            | CheckName::BitwardenClientId
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

/// verify-yubikey の local storage 検証と vault 外部確認の順序境界を port mock で検証する。
#[cfg(test)]
mod tests {
    use crate::secrets::{
        domain::{
            commands::VerifyYubikeyCommand,
            gpg_backup::{
                ConnectedYubiKey, EnvelopeCiphertext, EnvelopeMetadata, EnvelopeRecipient,
                GpgBackupEnvelope, PrimaryFingerprint,
            },
            manifest::SecretManifest,
            pass_restore::PasswordStoreRemote,
            piv::SecretName,
            storage::SecretStorageReadInspection,
            vault::{VaultLookupCandidate, VaultSecretId, VaultSecretName},
            verification::{CheckName, CheckStatus, ExternalCheck},
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::{VerifyYubikeyRuntime, run_verify_yubikey_with};

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

    /// vault check は local storage 検証後に master password を input port から 1 回だけ取得する。
    #[tokio::test]
    async fn verify_yubikey_vault_check_reads_master_password_after_local_storage_without_storage_write()
    -> crate::Result<()> {
        let serial = 7003;
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::yubikey::MockYubiKeyDevicePort::new();
        device
            .expect_resolve_device_serial()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(move || Ok(serial));
        device
            .expect_device_requires_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(false));
        let mut pin_input = ports::io::MockPinInputPort::new();
        pin_input.expect_read_pin().times(0);
        let mut storage = ports::yubikey::MockSecretStoragePort::new();
        forbid_yubikey_storage_writes(&mut storage);
        for name in [
            SecretName::BitwardenClientId,
            SecretName::BitwardenClientSecret,
            SecretName::BitwardenClientId,
            SecretName::BitwardenClientSecret,
        ] {
            expect_loaded_yubikey_secret(&mut storage, &mut sequence, serial, name);
        }
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
                Ok(vec![
                    VaultLookupCandidate {
                        id: VaultSecretId::new("gpg"),
                        name: VaultSecretName::GpgSecretKeyBackup.key().to_owned(),
                    },
                    VaultLookupCandidate {
                        id: VaultSecretId::new("pass"),
                        name: VaultSecretName::PasswordStoreRemote.key().to_owned(),
                    },
                ])
            });
        vault_client
            .expect_fetch_gpg_backup_envelope()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(move |_, _| gpg_backup_envelope(serial));
        let mut gpg_recipient = ports::yubikey::MockGpgRecipientPort::new();
        gpg_recipient
            .expect_resolve_connected_recipient()
            .times(1)
            .withf(move |actual_serial| *actual_serial == serial)
            .in_sequence(&mut sequence)
            .returning(move |_| connected_recipient(serial));
        gpg_recipient.expect_unwrap_dek().times(0);
        vault_client
            .expect_fetch_password_store_remote()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| PasswordStoreRemote::parse("git@github.com:owner/repo.git"));
        vault_client.expect_create_password_store_remote().times(0);
        let mut report = ports::io::MockReportPort::new();
        report
            .expect_write_verify_report()
            .times(1)
            .withf(|summary| {
                summary.checks.get(&CheckName::LocalStorage) == Some(&CheckStatus::Ok)
                    && summary.checks.get(&CheckName::Vault) == Some(&CheckStatus::Ok)
            })
            .in_sequence(&mut sequence)
            .returning(|_| Ok(()));

        run_verify_yubikey_with(
            VerifyYubikeyCommand {
                checks: vec![ExternalCheck::Vault],
                all: false,
            },
            VerifyYubikeyRuntime {
                device: &mut device,
                process: &pin_input,
                secret_input: &secret_input,
                storage: &mut storage,
                report: &report,
                vault_client: &vault_client,
                gpg_recipient: &mut gpg_recipient,
            },
        )
        .await?;

        Ok(())
    }
}
