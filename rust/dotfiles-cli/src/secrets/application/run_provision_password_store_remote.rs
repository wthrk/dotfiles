//! password-store-remote の保管側 provisioning 順序を固定し、入力/検証/create/update を port 境界へ閉じる。

use crate::Result;
use crate::secrets::{
    domain::{
        bws::{BwsProjectName, BwsSecretName},
        commands::ProvisionPasswordStoreRemoteCommand,
        piv::{SecretName, validate_piv_pin_len},
        storage::SecretStorageReadIntent,
    },
    ports,
};

/// private `password-store` repository の clone URL を BWS project `dotfiles-secret-recovery` へ
/// create または update する provisioning use case。
///
/// 設計「初期登録手順」step3（L113）が定める `password-store-remote` の保管コマンドを、`gpg-backup register`
/// と対称な順序制御として固定する。BWS access token を接続中 YubiKey storage から読み出し、project name から
/// project ID を解決（0件/複数件で停止）したうえで、既存 `password-store-remote` secret の有無を確認する。
/// 不在なら保護 buffer から読んだ clone URL を create し、ちょうど 1 件存在し上書き許可がある場合だけ
/// stale-overwrite 防止つきで update する。複数件は domain failure として停止する。
///
/// 順序を application に固定するのは「project/secret の解決と上書き確認を済ませてから値入力・保存へ進む」
/// 停止条件の責務境界を保護するためである。update 経路では値入力より前に更新確認を行い、拒否される更新で
/// hidden prompt 入力を発生させない。値（clone URL）と access token は argv / log / 永続ファイルへ出さず、
/// 入力は保護 buffer 経路（hidden prompt / pipe）から読み、URL 形式検証は domain rule へ委ねて port 境界の
/// protection 内で行う。stale-overwrite 防止は、更新前に取得した guard が更新直前の現行値と一致する場合だけ
/// 上書きする read-modify-write として port 契約に閉じる。
#[expect(
    clippy::too_many_arguments,
    reason = "provisioning は device/pin/storage/bws/secret-input/confirm の port を順序適用する単一 use case"
)]
pub(crate) async fn run_provision_password_store_remote<D, P, S, B, I, F>(
    command: ProvisionPasswordStoreRemoteCommand,
    device_serial: &mut D,
    pin_policy: &mut impl ports::DevicePinPolicyPort,
    process: &P,
    storage_port: &mut S,
    bws_client: &B,
    secret_input: &I,
    confirmation: &F,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    P: ports::PinInputPort,
    S: ports::SecretStoragePort,
    B: ports::BwsClientPort,
    I: ports::SecretInputPort,
    F: ports::BackupUpdateConfirmationPort,
{
    let serial = device_serial.resolve_device_serial(command.serial)?;
    let pin = if pin_policy.device_requires_pin(serial)? {
        let pin = process.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };

    // BWS access token を YubiKey storage から読み出し、復旧 project を解決する。
    let access_token = load_bws_access_token(serial, storage_port, pin.as_ref())?;
    let project_id = BwsProjectName::DOTFILES_SECRET_RECOVERY
        .resolve_id(bws_client.list_bws_projects(&access_token).await?)?;

    // 既存 password-store-remote secret の候補を取得する。0件は create、1件は update、複数件は停止。
    // `resolve_id` は 0件/複数件をどちらも `Err` にするため、create 経路と取り違えないよう同名候補
    // （`name == key`）の件数を直接数える。
    let key = BwsSecretName::PasswordStoreRemote.key();
    let candidates = bws_client
        .list_bws_secrets(&access_token, &project_id)
        .await?;
    let existing_count = candidates
        .iter()
        .filter(|candidate| candidate.name == key)
        .count();

    match existing_count {
        0 => {
            // 不在: 値を保護 buffer から読み、新規 create する。
            let value = secret_input.read_password_store_remote_secret()?;
            bws_client
                .create_password_store_remote(&access_token, &project_id, key, &value)
                .await
                .map(|_id| ())
        }
        1 => {
            // 存在: secret ID と stale-overwrite 防止 guard を取得し、更新確認を値入力より前に行う。
            let secret_id =
                BwsSecretName::PasswordStoreRemote.resolve_id(candidates, &project_id)?;
            let guard = bws_client
                .fetch_password_store_remote_guard(&access_token, &secret_id)
                .await?;
            let confirmed = confirmation.confirm_secret_overwrite(
                BwsProjectName::DOTFILES_SECRET_RECOVERY.as_str(),
                key,
                command.assume_overwrite,
            )?;
            if !confirmed {
                anyhow::bail!("password-store-remote secret overwrite was not confirmed");
            }
            let value = secret_input.read_password_store_remote_secret()?;
            bws_client
                .update_password_store_remote_if_unchanged(
                    &access_token,
                    &project_id,
                    &secret_id,
                    key,
                    &value,
                    &guard,
                )
                .await
        }
        _ => anyhow::bail!(
            "multiple password-store-remote secrets exist in the recovery project; refusing to provision"
        ),
    }
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
    //! provisioning の順序（token 取得→project 解決→secret 候補確認→create または確認付き guard update）を
    //! mockall + Sequence で検証する単体テスト。
    //!
    //! secret-input / bws / confirmation backend を port mock で差し替え、未登録時に確認・更新へ進ませず
    //! create が呼ばれること、ちょうど 1 件存在し確認を通過した場合だけ guard 付き update が呼ばれ、
    //! 確認は値入力より前に呼ばれること、確認拒否で値入力・update のいずれにも進ませないこと、同名複数件で
    //! create/update のいずれにも進ませず停止することを確認する。

    use crate::secrets::{
        domain::{
            commands::ProvisionPasswordStoreRemoteCommand, gpg_backup::BackupUpdateGuard,
            manifest::SecretManifest, storage::SecretStorageReadInspection,
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_provision_password_store_remote;

    const REMOTE_URL: &str = "git@github.com:owner/password-store.git";

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn read_inspection() -> SecretStorageReadInspection {
        SecretStorageReadInspection {
            manifest_bytes: Some(SecretManifest::expected().encode().expect("manifest")),
            encoded: Some(vec![1]),
        }
    }

    fn project_candidate() -> Vec<
        crate::secrets::domain::bws::BwsLookupCandidate<crate::secrets::domain::bws::BwsProjectId>,
    > {
        vec![crate::secrets::domain::bws::BwsLookupCandidate {
            id: crate::secrets::domain::bws::BwsProjectId::new("project-1"),
            name: "dotfiles-secret-recovery".to_owned(),
        }]
    }

    fn command(assume_overwrite: bool) -> ProvisionPasswordStoreRemoteCommand {
        ProvisionPasswordStoreRemoteCommand {
            serial: Some(2001),
            assume_overwrite,
        }
    }

    /// device/pin/storage の解決を共通設定する（PIN 不要・token 読み出し成功）。
    fn baseline_device_and_storage() -> (
        ports::MockDeviceSerialPort,
        ports::MockDevicePinPolicyPort,
        ports::MockPinInputPort,
        ports::MockSecretStoragePort,
    ) {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|requested| Ok(requested.expect("serial")));
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
        (device, pin_policy, process, storage)
    }

    #[tokio::test]
    async fn provision_creates_secret_when_absent() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let (mut device, mut pin_policy, process, mut storage) = baseline_device_and_storage();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        // 同名 secret は未登録（list は空）。create 経路へ進む。
        bws.expect_list_bws_secrets()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(Vec::new()));

        let mut secret_input = ports::MockSecretInputPort::new();
        secret_input
            .expect_read_password_store_remote_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(REMOTE_URL.as_bytes())));

        // 不在時は確認へ進ませない。
        let mut confirmation = ports::MockBackupUpdateConfirmationPort::new();
        confirmation.expect_confirm_secret_overwrite().times(0);

        bws.expect_create_password_store_remote()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _, _, _| Ok(crate::secrets::domain::bws::BwsSecretId::new("new-id")));
        bws.expect_update_password_store_remote_if_unchanged()
            .times(0);

        run_provision_password_store_remote(
            command(false),
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &bws,
            &secret_input,
            &confirmation,
        )
        .await
    }

    #[tokio::test]
    async fn provision_updates_with_guard_after_confirmation() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let (mut device, mut pin_policy, process, mut storage) = baseline_device_and_storage();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![crate::secrets::domain::bws::BwsLookupCandidate {
                id: crate::secrets::domain::bws::BwsSecretId::new("pass-id"),
                name: "password-store-remote".to_owned(),
            }])
        });
        bws.expect_fetch_password_store_remote_guard()
            .returning(|_, _| Ok(BackupUpdateGuard::ValueDigest("rev".to_owned())));

        // 確認は値入力より前に呼ばれる。
        let mut confirmation = ports::MockBackupUpdateConfirmationPort::new();
        confirmation
            .expect_confirm_secret_overwrite()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _, _| Ok(true));

        let mut secret_input = ports::MockSecretInputPort::new();
        secret_input
            .expect_read_password_store_remote_secret()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(material(REMOTE_URL.as_bytes())));

        bws.expect_update_password_store_remote_if_unchanged()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|_, _, _, _, _, guard| {
                *guard == BackupUpdateGuard::ValueDigest("rev".to_owned())
            })
            .returning(|_, _, _, _, _, _| Ok(()));
        bws.expect_create_password_store_remote().times(0);

        run_provision_password_store_remote(
            command(true),
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &bws,
            &secret_input,
            &confirmation,
        )
        .await
    }

    /// create 経路へ流れた値が URL 検証で不正なとき、`create_*` が返す検証由来 `Err` で use case が
    /// 停止することを検証する。
    ///
    /// 不正 URL の `Err` は domain rule [`PasswordStoreRemote::parse`] が実際に生成した値を mock から返し
    /// （real adapter / stub と同じ検証規則）、保存が成立せず create 経路でも provisioning が失敗で停止する
    /// 停止経路を駆動する。
    #[tokio::test]
    async fn provision_stops_when_create_rejects_invalid_url() {
        use crate::secrets::domain::pass_restore::PasswordStoreRemote;

        let (mut device, mut pin_policy, process, mut storage) = baseline_device_and_storage();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        // 同名 secret は未登録 → create 経路へ進む。
        bws.expect_list_bws_secrets()
            .returning(|_, _| Ok(Vec::new()));

        // create へ進む前に値入力は行われる（不在経路では確認なし）。
        let mut confirmation = ports::MockBackupUpdateConfirmationPort::new();
        confirmation.expect_confirm_secret_overwrite().times(0);

        // create 経路へ流す不正 clone URL（domain 妥当でない値）。
        const INVALID_URL: &str = "https://example.invalid/repo.git";
        let mut secret_input = ports::MockSecretInputPort::new();
        secret_input
            .expect_read_password_store_remote_secret()
            .times(1)
            .returning(|| Ok(material(INVALID_URL.as_bytes())));

        // create が URL 検証で停止する（real adapter / stub と同じ domain rule が不正 URL の `Err` を生成する）。
        bws.expect_create_password_store_remote()
            .times(1)
            .returning(|_, _, _, _| {
                PasswordStoreRemote::parse(INVALID_URL).map(|remote| {
                    crate::secrets::domain::bws::BwsSecretId::new(remote.as_str().to_owned())
                })
            });
        bws.expect_update_password_store_remote_if_unchanged()
            .times(0);

        let result = run_provision_password_store_remote(
            command(false),
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &bws,
            &secret_input,
            &confirmation,
        )
        .await;

        assert!(
            result.is_err(),
            "invalid clone URL must stop provisioning at the create path"
        );
    }

    /// 更新直前の現行値が変化していて guard 不一致になった場合、`update_*_if_unchanged` が返す
    /// stale-overwrite `Err` で use case 全体が停止することを検証する。
    ///
    /// guard 不一致の `Err` は domain rule [`BackupUpdateGuard::ensure_matches`] が実際に生成した値を
    /// mock から返し（テスト固有 message を捏造しない）、確認通過・値入力後でも保存が成立せず use case が
    /// 失敗で停止する停止経路を駆動する。
    #[tokio::test]
    async fn provision_stops_when_guard_mismatch_blocks_update() {
        let (mut device, mut pin_policy, process, mut storage) = baseline_device_and_storage();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![crate::secrets::domain::bws::BwsLookupCandidate {
                id: crate::secrets::domain::bws::BwsSecretId::new("pass-id"),
                name: "password-store-remote".to_owned(),
            }])
        });
        // 更新前に読んだ guard。
        bws.expect_fetch_password_store_remote_guard()
            .returning(|_, _| Ok(BackupUpdateGuard::ValueDigest("read-at-start".to_owned())));

        let mut confirmation = ports::MockBackupUpdateConfirmationPort::new();
        confirmation
            .expect_confirm_secret_overwrite()
            .times(1)
            .returning(|_, _, _| Ok(true));

        let mut secret_input = ports::MockSecretInputPort::new();
        secret_input
            .expect_read_password_store_remote_secret()
            .times(1)
            .returning(|| Ok(material(REMOTE_URL.as_bytes())));

        // 更新直前の現行値が更新前 guard と異なる → domain rule が stale-overwrite `Err` を生成する。
        bws.expect_update_password_store_remote_if_unchanged()
            .times(1)
            .returning(|_, _, _, _, _, expected_guard| {
                let current_guard = BackupUpdateGuard::ValueDigest("changed-since-read".to_owned());
                expected_guard.ensure_matches(&current_guard)
            });
        bws.expect_create_password_store_remote().times(0);

        let result = run_provision_password_store_remote(
            command(true),
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &bws,
            &secret_input,
            &confirmation,
        )
        .await;

        assert!(
            result.is_err(),
            "guard mismatch must stop provisioning with a stale-overwrite error"
        );
    }

    #[tokio::test]
    async fn provision_rejection_skips_value_input_and_update() {
        let (mut device, mut pin_policy, process, mut storage) = baseline_device_and_storage();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![crate::secrets::domain::bws::BwsLookupCandidate {
                id: crate::secrets::domain::bws::BwsSecretId::new("pass-id"),
                name: "password-store-remote".to_owned(),
            }])
        });
        bws.expect_fetch_password_store_remote_guard()
            .returning(|_, _| Ok(BackupUpdateGuard::ValueDigest("rev".to_owned())));
        // 拒否時は更新へ進ませない。
        bws.expect_update_password_store_remote_if_unchanged()
            .times(0);
        bws.expect_create_password_store_remote().times(0);

        // 拒否時は値入力を発生させない。
        let mut secret_input = ports::MockSecretInputPort::new();
        secret_input
            .expect_read_password_store_remote_secret()
            .times(0);

        let mut confirmation = ports::MockBackupUpdateConfirmationPort::new();
        confirmation
            .expect_confirm_secret_overwrite()
            .times(1)
            .returning(|_, _, _| Ok(false));

        let result = run_provision_password_store_remote(
            command(false),
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &bws,
            &secret_input,
            &confirmation,
        )
        .await;

        assert!(
            result.is_err(),
            "rejected overwrite must stop before value input and update"
        );
    }

    /// 同名 secret が複数件存在する場合は、create/update のいずれにも進ませず停止することを検証する。
    #[tokio::test]
    async fn provision_stops_when_duplicate_secrets_exist() {
        let (mut device, mut pin_policy, process, mut storage) = baseline_device_and_storage();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![
                crate::secrets::domain::bws::BwsLookupCandidate {
                    id: crate::secrets::domain::bws::BwsSecretId::new("dup-1"),
                    name: "password-store-remote".to_owned(),
                },
                crate::secrets::domain::bws::BwsLookupCandidate {
                    id: crate::secrets::domain::bws::BwsSecretId::new("dup-2"),
                    name: "password-store-remote".to_owned(),
                },
            ])
        });
        // 複数件検出で guard 取得・create・update のいずれにも進ませない。
        bws.expect_fetch_password_store_remote_guard().times(0);
        bws.expect_create_password_store_remote().times(0);
        bws.expect_update_password_store_remote_if_unchanged()
            .times(0);

        let mut secret_input = ports::MockSecretInputPort::new();
        secret_input
            .expect_read_password_store_remote_secret()
            .times(0);
        let mut confirmation = ports::MockBackupUpdateConfirmationPort::new();
        confirmation.expect_confirm_secret_overwrite().times(0);

        let result = run_provision_password_store_remote(
            command(true),
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &bws,
            &secret_input,
            &confirmation,
        )
        .await;

        assert!(
            result.is_err(),
            "duplicate password-store-remote secrets must stop provisioning"
        );
    }
}
