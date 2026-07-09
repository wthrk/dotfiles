//! gpg-secret-key-backup への spare recipient 追加順序を固定し、復号/再 wrap/更新を port 境界へ閉じる。

use crate::Result;
use crate::{
    domain::{
        bws::{BwsProjectName, BwsSecretName},
        commands::AddGpgBackupSpareCommand,
        gpg_backup::EnvelopeRecipient,
        piv::validate_piv_pin_len,
    },
    ports,
};

/// `run_add_gpg_backup_spare` が使う外部 capability を named field で束ねる。
pub(crate) struct AddGpgBackupSpareRuntime<'a, B> {
    pub(crate) token_input: &'a dyn ports::BitwardenClientSecretInputPort,
    pub(crate) device: &'a mut dyn ports::YubiKeyDevicePort,
    pub(crate) spare_device_serial: &'a mut dyn ports::SpareDeviceSerialPort,
    pub(crate) process: &'a dyn ports::PinInputPort,
    pub(crate) bws_client: &'a B,
    pub(crate) recipient: &'a mut dyn ports::GpgRecipientPort,
    pub(crate) confirmation: &'a dyn ports::BackupUpdateConfirmationPort,
}

/// 既存 envelope を復号して同一 DEK を取り出し、spare YubiKey の recipient を追加して BWS を更新する。
///
/// 設計「recipient 運用 / BWS 更新契約」の spare 追加経路を順序制御として固定する。既存 recipient 機
/// （unwrap 機）で envelope の DEK を unwrap し、同一 DEK を spare の PIV slot `82` 公開鍵で再 wrap して
/// recipient を追加する。`ciphertext` と `metadata` は変更しない。更新は read-modify-write として扱い、
/// 更新前に取得した stale overwrite 防止 guard が更新直前の現行値と一致する場合だけ上書きする。対話実行は
/// 明示確認後、非対話実行は明示的上書き許可がある場合だけ更新する。
///
/// BWS への更新には client-secret を使う。この更新用 token は hidden prompt / pipe から
/// `BitwardenClientSecretInputPort` 経由で取得し、YubiKey へ保存しない。YubiKey へ保存する `bitwarden-client-secret` は
/// 復旧時の read 用最小権限 token を別経路で用意する。この provisioning command 自体は YubiKey storage
/// 経由の token 読み出しを行わない。一方、YubiKey 本体は既存 recipient による DEK unwrap（PIV slot `82` 秘密鍵、
/// PIN/touch を要する）と spare recipient wrap に必要なため、unwrap 機・spare 機の
/// device serial 解決と PIN 入力は残す。
///
/// 順序を application に固定するのは「envelope 取得後に更新確認を済ませてから PIN 取得・DEK unwrap・spare wrap を
/// 行い、guard 一致を条件に更新する」停止条件の責務境界を保護するためである。更新確認を PIN 取得・unwrap/wrap より
/// 前へ置くのは、拒否される更新で YubiKey の PIN prompt・touch と DEK 復号を一切発生させないためである。device serial
/// 解決（YubiKey 識別）は PIN を伴わないため確認より前で済ませるが、PIN prompt 自体は確認成功後・`unwrap_dek` 直前まで
/// 遅らせる。DEK は port 境界の保護値として扱い、application 層では加工しない。
pub(crate) async fn run_add_gpg_backup_spare<B>(
    command: AddGpgBackupSpareCommand,
    runtime: AddGpgBackupSpareRuntime<'_, B>,
) -> Result<()>
where
    B: ports::BwsClientPort,
{
    let AddGpgBackupSpareRuntime {
        token_input,
        device,
        spare_device_serial,
        process,
        bws_client,
        recipient,
        confirmation,
    } = runtime;
    command.ensure_requested_serials_distinct()?;
    let unwrap_serial = device.resolve_device_serial(command.unwrap_serial)?;
    let spare_serial = spare_device_serial.resolve_spare_device_serial(command.spare_serial)?;
    command.ensure_distinct_resolved_serials(unwrap_serial, spare_serial)?;

    // device serial 解決（YubiKey 識別）は PIN を伴わないため確認より前で済ませてよいが、PIN prompt 自体は
    // DEK unwrap のためだけに必要であり、上書きを拒否する対話ケースでは発生させない。よって PIN 取得は
    // `confirm_backup_update` 成功後・`unwrap_dek` 直前まで遅らせる。

    // BWS 更新用 access token を hidden prompt / pipe から取得し、復旧 project / secret を解決する。
    // provisioning command は YubiKey storage を読まず、YubiKey 保存用の復旧 token とは分離する。
    let access_token = token_input.read_bitwarden_client_secret_for_provisioning()?;
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

    // unwrap/wrap で PIN/touch と DEK 復号を発生させる前に更新確認を行う。確認に必要な fingerprint は
    // envelope 取得後に判明しているため、拒否される更新で YubiKey の DEK unwrap や spare wrap を実行
    // しないよう、確認を unwrap/wrap より前へ置く。
    let confirmed = confirmation.confirm_backup_update(
        BwsProjectName::DOTFILES_SECRET_RECOVERY.as_str(),
        key,
        envelope.metadata().primary_fingerprint().as_str(),
        command.assume_overwrite,
    )?;
    if !confirmed {
        anyhow::bail!("gpg backup spare recipient update was not confirmed");
    }

    // 確認を通過したので、DEK unwrap のためにのみ必要な PIN をここで初めて取得する。上書き拒否時は
    // この経路に到達せず、YubiKey の PIN/touch は一切発生しない。
    let pin = if device.device_requires_pin(unwrap_serial)? {
        let pin = process.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };

    // unwrap 機が既存 recipient に一致することを確認し、DEK を unwrap する。
    let connected = recipient.resolve_connected_recipient(unwrap_serial)?;
    let matched = envelope.resolve_recipient(&connected)?;
    let dek = recipient.unwrap_dek(unwrap_serial, matched, pin.as_ref())?;

    // 同一 DEK を spare の公開鍵で再 wrap し、recipient を追加した新しい envelope を作る。
    let spare_recipient: EnvelopeRecipient =
        recipient.wrap_dek_for_recipient(spare_serial, &dek)?;
    let updated = envelope.with_added_recipient(spare_recipient)?;

    bws_client
        .update_gpg_backup_envelope_if_unchanged(
            &access_token,
            &project_id,
            &secret_id,
            &updated,
            &guard,
        )
        .await
}

#[cfg(test)]
mod tests {
    //! spare 追加の順序（device 解決→client-secret 取得→envelope 取得→確認→unwrap→再 wrap→
    //! guard 更新）を mockall + Sequence で検証する単体テスト。
    //!
    //! token-input / recipient / bws / confirmation backend を port mock で差し替え、BWS 更新に使う
    //! access token を client-secret 入力経路から取得すること、YubiKey は recipient unwrap/wrap にのみ使うこと、
    //! 確認が PIN 取得・unwrap/wrap より前に呼ばれ、確認を通過した場合だけ確認→PIN→unwrap→wrap→guard 付き更新の
    //! 順で進むこと、確認拒否時に PIN 取得・DEK unwrap・spare wrap・更新のいずれにも進ませないことを確認する。

    use crate::{
        domain::{
            commands::AddGpgBackupSpareCommand,
            gpg_backup::{
                BackupUpdateGuard, ConnectedYubiKey, EnvelopeRecipient, GpgBackupEnvelope,
            },
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::{AddGpgBackupSpareRuntime, run_add_gpg_backup_spare};

    const PRIMARY_FP: &str = "0123456789abcdef0123456789abcdef01234567";

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    /// client-secret を hidden prompt / pipe から取得する port mock を共通設定する。
    ///
    /// この mock は hidden prompt / pipe 相当の入力経路として client-secret を返す。
    /// storage port は構成へ一切渡さず、provisioning command が YubiKey storage を読まないことを固定する。
    fn token_input() -> ports::MockBitwardenClientSecretInputPort {
        let mut token_input = ports::MockBitwardenClientSecretInputPort::new();
        token_input
            .expect_read_bitwarden_client_secret_for_provisioning()
            .times(1)
            .returning(|| Ok(material(b"provisioning-token")));
        token_input
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
        let mut sequence = mockall::Sequence::new();
        let token = token_input();
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut spare = ports::MockSpareDeviceSerialPort::new();
        spare
            .expect_resolve_spare_device_serial()
            .returning(|_| Ok(2002));
        // PIN を要求する機材を模し、PIN 取得が確認成功後に限定されることを順序で固定する。
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .returning(|_| Ok(true));
        let mut process = ports::MockPinInputPort::new();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().returning(|_| {
            Ok(vec![crate::domain::bws::BwsLookupCandidate {
                id: crate::domain::bws::BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![crate::domain::bws::BwsLookupCandidate {
                id: crate::domain::bws::BwsSecretId::new("gpg-id"),
                name: "gpg-secret-key-backup".to_owned(),
            }])
        });
        bws.expect_fetch_gpg_backup_envelope()
            .returning(|_, _| Ok((envelope(), BackupUpdateGuard::ValueDigest("rev".to_owned()))));

        // 確認は PIN 取得・unwrap/wrap より前に呼ばれる。
        let mut confirmation = ports::MockBackupUpdateConfirmationPort::new();
        confirmation
            .expect_confirm_backup_update()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _, _, _| Ok(true));

        // PIN 取得は確認成功後・unwrap 直前に発生する。
        process
            .expect_read_pin()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(b"123456")));

        let mut recipient = ports::MockGpgRecipientPort::new();
        recipient
            .expect_resolve_connected_recipient()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(connected_unwrap()));
        recipient
            .expect_unwrap_dek()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _, _| Ok(material(b"dek")));
        recipient
            .expect_wrap_dek_for_recipient()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(spare_recipient()));

        bws.expect_update_gpg_backup_envelope_if_unchanged()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|_, _, _, envelope, guard| {
                envelope.recipients().len() == 2
                    && *guard == BackupUpdateGuard::ValueDigest("rev".to_owned())
            })
            .returning(|_, _, _, _, _| Ok(()));

        run_add_gpg_backup_spare(
            AddGpgBackupSpareCommand {
                unwrap_serial: Some(2001),
                spare_serial: Some(2002),
                assume_overwrite: true,
            },
            AddGpgBackupSpareRuntime {
                token_input: &token,
                device: &mut (&mut device, &mut pin_policy),
                spare_device_serial: &mut spare,
                process: &process,
                bws_client: &bws,
                recipient: &mut recipient,
                confirmation: &confirmation,
            },
        )
        .await
    }

    /// 更新直前の現行 envelope が変化していて guard 不一致になった場合、stale overwrite `Err` で停止する。
    ///
    /// guard 不一致の `Err` は domain rule [`BackupUpdateGuard::ensure_matches`] が実際に生成した値を
    /// mock から返し、確認通過・PIN 取得・DEK unwrap・spare wrap 後でも保存成立として扱わない停止経路を
    /// 固定する。
    #[tokio::test]
    async fn add_spare_stops_when_guard_mismatch_blocks_update() {
        let token = token_input();
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
            .returning(|_| Ok(true));
        let mut process = ports::MockPinInputPort::new();
        process
            .expect_read_pin()
            .times(1)
            .returning(|| Ok(material(b"123456")));

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().returning(|_| {
            Ok(vec![crate::domain::bws::BwsLookupCandidate {
                id: crate::domain::bws::BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![crate::domain::bws::BwsLookupCandidate {
                id: crate::domain::bws::BwsSecretId::new("gpg-id"),
                name: "gpg-secret-key-backup".to_owned(),
            }])
        });
        bws.expect_fetch_gpg_backup_envelope().returning(|_, _| {
            Ok((
                envelope(),
                BackupUpdateGuard::ValueDigest("read-at-start".to_owned()),
            ))
        });
        bws.expect_update_gpg_backup_envelope_if_unchanged()
            .times(1)
            .returning(|_, _, _, _, expected_guard| {
                let current_guard = BackupUpdateGuard::ValueDigest("changed-since-read".to_owned());
                expected_guard.ensure_matches(&current_guard)
            });

        let mut recipient = ports::MockGpgRecipientPort::new();
        recipient
            .expect_resolve_connected_recipient()
            .times(1)
            .returning(|_| Ok(connected_unwrap()));
        recipient
            .expect_unwrap_dek()
            .times(1)
            .returning(|_, _, _| Ok(material(b"dek")));
        recipient
            .expect_wrap_dek_for_recipient()
            .times(1)
            .returning(|_, _| Ok(spare_recipient()));

        let mut confirmation = ports::MockBackupUpdateConfirmationPort::new();
        confirmation
            .expect_confirm_backup_update()
            .times(1)
            .returning(|_, _, _, _| Ok(true));

        let result = run_add_gpg_backup_spare(
            AddGpgBackupSpareCommand {
                unwrap_serial: Some(2001),
                spare_serial: Some(2002),
                assume_overwrite: true,
            },
            AddGpgBackupSpareRuntime {
                token_input: &token,
                device: &mut (&mut device, &mut pin_policy),
                spare_device_serial: &mut spare,
                process: &process,
                bws_client: &bws,
                recipient: &mut recipient,
                confirmation: &confirmation,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "guard mismatch must stop spare recipient update with a stale-overwrite error"
        );
    }

    /// 確認が拒否された場合、PIN 取得・DEK unwrap・spare wrap・guard 更新のいずれにも進ませず、
    /// YubiKey の PIN/touch と DEK 復号を発生させないことを検証する。
    #[tokio::test]
    async fn add_spare_rejection_skips_unwrap_and_update() {
        let token = token_input();
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
        let mut spare = ports::MockSpareDeviceSerialPort::new();
        spare
            .expect_resolve_spare_device_serial()
            .returning(|_| Ok(2002));
        // PIN を要求する機材でも、確認拒否時は PIN 取得へ到達しないことを検証する。
        let mut pin_policy = ports::MockDevicePinPolicyPort::new();
        pin_policy
            .expect_device_requires_pin()
            .returning(|_| Ok(true));
        let mut process = ports::MockPinInputPort::new();
        // 拒否時は PIN prompt を発生させない。
        process.expect_read_pin().times(0);

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().returning(|_| {
            Ok(vec![crate::domain::bws::BwsLookupCandidate {
                id: crate::domain::bws::BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![crate::domain::bws::BwsLookupCandidate {
                id: crate::domain::bws::BwsSecretId::new("gpg-id"),
                name: "gpg-secret-key-backup".to_owned(),
            }])
        });
        bws.expect_fetch_gpg_backup_envelope()
            .returning(|_, _| Ok((envelope(), BackupUpdateGuard::ValueDigest("rev".to_owned()))));
        // 拒否時は更新へ進ませない。
        bws.expect_update_gpg_backup_envelope_if_unchanged()
            .times(0);

        // 拒否時は DEK unwrap も spare wrap も発生させない。
        let mut recipient = ports::MockGpgRecipientPort::new();
        recipient.expect_resolve_connected_recipient().times(0);
        recipient.expect_unwrap_dek().times(0);
        recipient.expect_wrap_dek_for_recipient().times(0);

        let mut confirmation = ports::MockBackupUpdateConfirmationPort::new();
        confirmation
            .expect_confirm_backup_update()
            .times(1)
            .returning(|_, _, _, _| Ok(false));

        let result = run_add_gpg_backup_spare(
            AddGpgBackupSpareCommand {
                unwrap_serial: Some(2001),
                spare_serial: Some(2002),
                assume_overwrite: false,
            },
            AddGpgBackupSpareRuntime {
                token_input: &token,
                device: &mut (&mut device, &mut pin_policy),
                spare_device_serial: &mut spare,
                process: &process,
                bws_client: &bws,
                recipient: &mut recipient,
                confirmation: &confirmation,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "rejected spare update must stop before unwrap and update"
        );
    }
}
