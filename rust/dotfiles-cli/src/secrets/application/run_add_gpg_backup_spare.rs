//! gpg-secret-key-backup への spare recipient 追加順序を固定し、復号/再 wrap/更新を port 境界へ閉じる。

use crate::Result;
use crate::secrets::{
    domain::{
        bws::{BwsProjectName, BwsSecretName},
        commands::AddGpgBackupSpareCommand,
        gpg_backup::EnvelopeRecipient,
        piv::{SecretName, validate_piv_pin_len},
        storage::SecretStorageReadIntent,
    },
    ports,
};

/// 既存 envelope を復号して同一 DEK を取り出し、spare YubiKey の recipient を追加して BWS を更新する。
///
/// 設計「recipient 運用 / BWS 更新契約」の spare 追加経路を順序制御として固定する。既存 recipient 機
/// （unwrap 機）で envelope の DEK を unwrap し、同一 DEK を spare の PIV slot `82` 公開鍵で再 wrap して
/// recipient を追加する。`ciphertext` と `metadata` は変更しない。更新は read-modify-write として扱い、
/// 更新前に取得した stale overwrite 防止 guard が更新直前の現行値と一致する場合だけ上書きする。対話実行は
/// 明示確認後、非対話実行は明示的上書き許可がある場合だけ更新する。順序を application に固定するのは
/// 「DEK を unwrap し新 recipient を追加してから guard 一致を確認して更新する」停止条件の責務境界を保護
/// するためである。DEK は port 境界の保護値として扱い、application 層では加工しない。
#[expect(
    clippy::too_many_arguments,
    reason = "spare 追加は device/spare-device/pin/storage/bws/recipient/confirm の port を順序適用する単一 use case"
)]
pub(crate) async fn run_add_gpg_backup_spare<D, SD, P, S, B, Y, F>(
    command: AddGpgBackupSpareCommand,
    device_serial: &mut D,
    spare_device_serial: &mut SD,
    pin_policy: &mut impl ports::DevicePinPolicyPort,
    process: &P,
    storage_port: &mut S,
    bws_client: &B,
    recipient: &mut Y,
    confirmation: &F,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    SD: ports::SpareDeviceSerialPort,
    P: ports::PinInputPort,
    S: ports::SecretStoragePort,
    B: ports::BwsClientPort,
    Y: ports::GpgRecipientPort,
    F: ports::BackupUpdateConfirmationPort,
{
    command.ensure_requested_serials_distinct()?;
    let unwrap_serial = device_serial.resolve_device_serial(command.unwrap_serial)?;
    let spare_serial = spare_device_serial.resolve_spare_device_serial(command.spare_serial)?;
    command.ensure_distinct_resolved_serials(unwrap_serial, spare_serial)?;

    let pin = if pin_policy.device_requires_pin(unwrap_serial)? {
        let pin = process.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };

    // unwrap 機の storage から bws-access-token を読み出し、復旧 project / secret を解決する。
    let access_token = load_bws_access_token(unwrap_serial, storage_port, pin.as_ref())?;
    let project_id = BwsProjectName::DOTFILES_SECRET_RECOVERY
        .resolve_id(bws_client.list_bws_projects(&access_token).await?)?;
    let key = BwsSecretName::GpgSecretKeyBackup.key();
    let secret_id = BwsSecretName::GpgSecretKeyBackup.resolve_id(
        bws_client
            .list_bws_secrets(&access_token, &project_id)
            .await?,
        &project_id,
    )?;

    // 更新前に envelope と stale overwrite 防止 guard を取得する。
    let (envelope, guard) = bws_client
        .fetch_gpg_backup_envelope(&access_token, &secret_id)
        .await?;

    // unwrap 機が既存 recipient に一致することを確認し、DEK を unwrap する。
    let connected = recipient.resolve_connected_recipient(unwrap_serial)?;
    let matched = envelope.resolve_recipient(&connected)?;
    let dek = recipient.unwrap_dek(unwrap_serial, matched, pin.as_ref())?;

    // 同一 DEK を spare の公開鍵で再 wrap し、recipient を追加した新しい envelope を作る。
    let spare_recipient: EnvelopeRecipient =
        recipient.wrap_dek_for_recipient(spare_serial, &dek)?;
    let updated = envelope.with_added_recipient(spare_recipient)?;

    // 対話確認（非対話は明示許可）を通った場合だけ guard 一致を条件に上書きする。
    let confirmed = confirmation.confirm_backup_update(
        BwsProjectName::DOTFILES_SECRET_RECOVERY.as_str(),
        key,
        envelope.metadata().primary_fingerprint().as_str(),
        command.assume_overwrite,
    )?;
    if !confirmed {
        anyhow::bail!("gpg backup spare recipient update was not confirmed");
    }
    bws_client
        .update_gpg_backup_envelope_if_unchanged(
            &access_token,
            &project_id,
            &secret_id,
            key,
            &updated,
            &guard,
        )
        .await
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
    //! spare 追加の順序（device 解決→token 取得→envelope 取得→unwrap→再 wrap→確認→guard 更新）を
    //! mockall + Sequence で検証する単体テスト。
    //!
    //! recipient / bws / confirmation backend を port mock で差し替え、確認を通過した場合だけ guard 付き
    //! 更新が呼ばれること、確認拒否時に更新へ進ませないことを確認する。

    use crate::secrets::{
        domain::{
            commands::AddGpgBackupSpareCommand,
            gpg_backup::{
                BackupUpdateGuard, ConnectedYubiKey, EnvelopeRecipient, GpgBackupEnvelope,
            },
            manifest::SecretManifest,
            storage::SecretStorageReadInspection,
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_add_gpg_backup_spare;

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

    /// unwrap 機 serial 2001 に一致する recipient を 1 件持つ envelope を作る。
    fn envelope() -> GpgBackupEnvelope {
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
        GpgBackupEnvelope::parse(&json).expect("envelope")
    }

    fn connected_unwrap() -> ConnectedYubiKey {
        ConnectedYubiKey::new(
            "2001",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .expect("connected")
    }

    fn spare_recipient() -> EnvelopeRecipient {
        let connected = ConnectedYubiKey::new(
            "2002",
            "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210",
        )
        .expect("spare connected");
        EnvelopeRecipient::new(&connected, b"wrapped-spare".to_vec()).expect("spare recipient")
    }

    #[tokio::test]
    async fn add_spare_updates_with_guard_after_confirmation() -> crate::Result<()> {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut spare = ports::MockSpareDeviceSerialPort::new();
        spare
            .expect_resolve_spare_device_serial()
            .returning(|_| Ok(2002));
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
            .returning(|_, _| Ok((envelope(), BackupUpdateGuard::ValueDigest("rev".to_owned()))));
        bws.expect_update_gpg_backup_envelope_if_unchanged()
            .times(1)
            .withf(|_, _, _, _, envelope, guard| {
                envelope.recipients().len() == 2
                    && *guard == BackupUpdateGuard::ValueDigest("rev".to_owned())
            })
            .returning(|_, _, _, _, _, _| Ok(()));

        let mut recipient = ports::MockGpgRecipientPort::new();
        recipient
            .expect_resolve_connected_recipient()
            .returning(|_| Ok(connected_unwrap()));
        recipient
            .expect_unwrap_dek()
            .returning(|_, _, _| Ok(material(b"dek")));
        recipient
            .expect_wrap_dek_for_recipient()
            .returning(|_, _| Ok(spare_recipient()));

        let mut confirmation = ports::MockBackupUpdateConfirmationPort::new();
        confirmation
            .expect_confirm_backup_update()
            .times(1)
            .returning(|_, _, _, _| Ok(true));

        run_add_gpg_backup_spare(
            AddGpgBackupSpareCommand {
                unwrap_serial: Some(2001),
                spare_serial: Some(2002),
                assume_overwrite: true,
            },
            &mut device,
            &mut spare,
            &mut pin_policy,
            &process,
            &mut storage,
            &bws,
            &mut recipient,
            &confirmation,
        )
        .await
    }
}
