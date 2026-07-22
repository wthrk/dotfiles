//! password-store-remote の保管側 provisioning 順序を固定し、入力/検証/create/update を port 境界へ閉じる。

use crate::Result;
use crate::{
    domain::{
        bws::{BwsProjectName, BwsSecretName},
        commands::ProvisionPasswordStoreRemoteCommand,
        pass_restore::PasswordStoreRemote,
        piv::SecretName,
        storage::SecretStorageReadIntent,
    },
    ports,
};

/// private `password-store` repository の clone URL を BWS project `dotfiles-secret-recovery` へ
/// create または update する provisioning use case。
///
/// 設計「初期登録手順」step3 が定める `password-store-remote` の保管コマンドを、`gpg-backup register`
/// と対称な順序制御として固定する。BWS への登録には YubiKey storage の `bitwarden-client-secret` を
/// 使い、明示 serial または単一接続 device から token を読み出したうえで、project name から project ID
/// を解決（0件/複数件で停止）し、既存 `password-store-remote` secret の有無を確認する。不在なら入力した
/// clone URL を create し、ちょうど 1 件存在し上書き許可がある場合だけ stale-overwrite 防止つきで update する。
/// 複数件は domain failure として停止する。BWS token は prompt / stdin から受け取らず、YubiKey へ
/// 保存・更新する経路（`yubikey put` / `enroll-primary` / `rotate-bws-token`）だけが入力を担う。
///
/// 順序を application に固定するのは「token 取得・project/secret の解決と上書き確認を済ませてから値入力・
/// 保存へ進む」停止条件の責務境界を保護するためである。update 経路では値入力より前に更新確認を行い、
/// 拒否される更新で余計な URL 入力を発生させない。clone URL は private repo の SSH clone URL であって秘密
/// 情報ではないため、保護 buffer・非表示入力・zeroize を使わず非秘匿の値として扱う。入力は優先順位
/// 「`--url` 指定値 ＞（未指定時）`PasswordStoreRemoteInputPort` 経由の可視プロンプト（terminal）/ pipe
/// （非 terminal）」で得て、`PasswordStoreRemote::parse` の URL 形式検証（domain rule）を通してから
/// create/update へ渡す。stale-overwrite 防止は、更新前に取得した guard が更新直前の現行値と一致する場合
/// だけ上書きする read-modify-write として port 契約に閉じる。
pub(crate) async fn run_provision_password_store_remote<B, U, F>(
    command: ProvisionPasswordStoreRemoteCommand,
    device: &mut dyn ports::DeviceSerialPort,
    storage_port: &mut dyn ports::SecretStoragePort,
    bws_client: &B,
    url_input: &U,
    confirmation: &F,
) -> Result<()>
where
    B: ports::BwsClientPort + ?Sized,
    U: ports::PasswordStoreRemoteInputPort + ?Sized,
    F: ports::BackupUpdateConfirmationPort + ?Sized,
{
    let serial = device.resolve_device_serial(command.serial)?;

    // BWS 登録・更新用 access token は YubiKey storage の `bitwarden-client-secret` から取得する。
    let access_token = load_bitwarden_client_secret(serial, storage_port)?;
    let project_id = BwsProjectName::DOTFILES_SECRET_RECOVERY
        .resolve_id(bws_client.list_bws_projects(&access_token).await?)?;

    // 既存 password-store-remote secret の候補を取得する。0件は create、1件は update、複数件は停止。
    // `resolve_id` は 0件/複数件をどちらも `Err` にするため、create 経路と取り違えないよう同名候補の
    // 件数を数える。対象同一性の exact match は domain helper に委ね、application は create/update/停止の
    // 分岐だけを扱う。
    let secret_name = BwsSecretName::PasswordStoreRemote;
    let candidates = bws_client
        .list_bws_secrets(&access_token, &project_id)
        .await?;
    let existing_count = candidates
        .iter()
        .filter(|candidate| secret_name.matches_candidate(candidate))
        .count();

    match existing_count {
        0 => {
            // 不在: clone URL を入力（--url ＞ 可視プロンプト/pipe）し、検証してから新規 create する。
            let remote = resolve_remote_url(&command.url, url_input)?;
            bws_client
                .create_password_store_remote(
                    &access_token,
                    &project_id,
                    secret_name.key(),
                    &remote,
                )
                .await
                .map(|_id| ())
        }
        1 => {
            // 存在: secret ID と stale-overwrite 防止 guard を取得し、更新確認を値入力より前に行う。
            let secret_id = secret_name.resolve_id(candidates, &project_id)?;
            let guard = bws_client
                .fetch_password_store_remote_guard(&access_token, &secret_id)
                .await?;
            let confirmed = confirmation.confirm_secret_overwrite(
                BwsProjectName::DOTFILES_SECRET_RECOVERY.as_str(),
                secret_name.key(),
                command.assume_overwrite,
            )?;
            if !confirmed {
                anyhow::bail!("password-store-remote secret overwrite was not confirmed");
            }
            let remote = resolve_remote_url(&command.url, url_input)?;
            bws_client
                .update_password_store_remote_if_unchanged(
                    &access_token,
                    &project_id,
                    &secret_id,
                    secret_name.key(),
                    &remote,
                    &guard,
                )
                .await
        }
        _ => anyhow::bail!(
            "multiple password-store-remote secrets exist in the recovery project; refusing to provision"
        ),
    }
}

/// clone URL を入力経路の優先順位に従って取得し、domain rule で検証した値を返す。
///
/// 優先順位は「`--url` で明示指定された値 ＞ （未指定時）`PasswordStoreRemoteInputPort` の可視プロンプト
/// （terminal）/ pipe（非 terminal）」とする。clone URL は秘密情報ではないため `String` で運び、
/// [`PasswordStoreRemote::parse`] による形式検証（`git@github.com:<owner>/<repo>.git`）だけを domain rule に
/// 委ねる。検証失敗は停止条件として呼び出し元へ伝播する。
fn resolve_remote_url<U>(url: &Option<String>, url_input: &U) -> Result<PasswordStoreRemote>
where
    U: ports::PasswordStoreRemoteInputPort + ?Sized,
{
    let raw = match url {
        Some(value) => value.clone(),
        None => url_input.read_password_store_remote_url()?,
    };
    PasswordStoreRemote::parse(&raw)
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
    //! provisioning の順序（YubiKey token 取得→project 解決→secret 候補確認→create または確認付き
    //! guard update）を mockall + Sequence で検証する単体テスト。
    //!
    //! YubiKey storage / url-input / bws / confirmation backend を port mock で差し替え、BWS 登録に使う
    //! access token を YubiKey storage の `bitwarden-client-secret` から取得すること、未登録時に確認・更新へ進ませず create が呼ばれること、
    //! ちょうど 1 件存在し確認を通過した場合だけ guard 付き update が呼ばれ、確認は値入力より前に呼ばれること、
    //! 確認拒否で値入力・update のいずれにも進ませないこと、同名複数件で create/update のいずれにも進ませず
    //! 停止すること、`--url` 指定値・可視プロンプト/pipe 入力のいずれの経路でも検証済み URL が create/update へ
    //! 渡ること、不正 URL が create/update より前に停止することを確認する。

    use crate::{
        domain::{
            bws::BwsSecretId, commands::ProvisionPasswordStoreRemoteCommand,
            gpg_backup::BackupUpdateGuard, manifest::SecretManifest,
            pass_restore::PasswordStoreRemote, storage::SecretStorageReadInspection,
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_provision_password_store_remote;

    const REMOTE_URL: &str = "git@github.com:owner/password-store.git";

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn project_candidate()
    -> Vec<crate::domain::bws::BwsLookupCandidate<crate::domain::bws::BwsProjectId>> {
        vec![crate::domain::bws::BwsLookupCandidate {
            id: crate::domain::bws::BwsProjectId::new("project-1"),
            name: "dotfiles-secret-recovery".to_owned(),
        }]
    }

    fn command(assume_overwrite: bool) -> ProvisionPasswordStoreRemoteCommand {
        ProvisionPasswordStoreRemoteCommand {
            assume_overwrite,
            serial: Some(2001),
            url: None,
        }
    }

    fn command_with_url(assume_overwrite: bool, url: &str) -> ProvisionPasswordStoreRemoteCommand {
        ProvisionPasswordStoreRemoteCommand {
            assume_overwrite,
            serial: Some(2001),
            url: Some(url.to_owned()),
        }
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

    #[tokio::test]
    async fn provision_creates_secret_when_absent() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut yk = yubikey_access_mocks();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        // 同名 secret は未登録（list は空）。create 経路へ進む。
        bws.expect_list_bws_secrets()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(Vec::new()));

        // 可視プロンプト/pipe 経路（--url 未指定）で URL を入力する。
        let mut url_input = ports::MockPasswordStoreRemoteInputPort::new();
        url_input
            .expect_read_password_store_remote_url()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(REMOTE_URL.to_owned()));

        // 不在時は確認へ進ませない。
        let mut confirmation = ports::MockBackupUpdateConfirmationPort::new();
        confirmation.expect_confirm_secret_overwrite().times(0);

        bws.expect_create_password_store_remote()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|_, _, key, remote| {
                key == "password-store-remote" && remote.as_str() == REMOTE_URL
            })
            .returning(|_, _, _, _| Ok(BwsSecretId::new("new-id")));
        bws.expect_update_password_store_remote_if_unchanged()
            .times(0);

        run_provision_password_store_remote(
            command(false),
            &mut yk.device,
            &mut yk.storage,
            &bws,
            &url_input,
            &confirmation,
        )
        .await
    }

    /// `--url` 指定値で create するとき、可視プロンプト/pipe 入力 port を呼ばずに引数値を使うことを検証する。
    #[tokio::test]
    async fn provision_creates_secret_from_url_argument() -> crate::Result<()> {
        let mut yk = yubikey_access_mocks();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        bws.expect_list_bws_secrets()
            .returning(|_, _| Ok(Vec::new()));

        // --url 指定時は port 入力を呼ばない。
        let mut url_input = ports::MockPasswordStoreRemoteInputPort::new();
        url_input.expect_read_password_store_remote_url().times(0);

        let mut confirmation = ports::MockBackupUpdateConfirmationPort::new();
        confirmation.expect_confirm_secret_overwrite().times(0);

        bws.expect_create_password_store_remote()
            .times(1)
            .withf(|_, _, key, remote: &PasswordStoreRemote| {
                key == "password-store-remote" && remote.as_str() == REMOTE_URL
            })
            .returning(|_, _, _, _| Ok(BwsSecretId::new("new-id")));
        bws.expect_update_password_store_remote_if_unchanged()
            .times(0);

        run_provision_password_store_remote(
            command_with_url(false, REMOTE_URL),
            &mut yk.device,
            &mut yk.storage,
            &bws,
            &url_input,
            &confirmation,
        )
        .await
    }

    #[tokio::test]
    async fn provision_updates_with_guard_after_confirmation() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut yk = yubikey_access_mocks();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![crate::domain::bws::BwsLookupCandidate {
                id: crate::domain::bws::BwsSecretId::new("pass-id"),
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

        let mut url_input = ports::MockPasswordStoreRemoteInputPort::new();
        url_input
            .expect_read_password_store_remote_url()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(REMOTE_URL.to_owned()));

        bws.expect_update_password_store_remote_if_unchanged()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|_, _, _, key, remote: &PasswordStoreRemote, guard| {
                key == "password-store-remote"
                    && remote.as_str() == REMOTE_URL
                    && *guard == BackupUpdateGuard::ValueDigest("rev".to_owned())
            })
            .returning(|_, _, _, _, _, _| Ok(()));
        bws.expect_create_password_store_remote().times(0);

        run_provision_password_store_remote(
            command(true),
            &mut yk.device,
            &mut yk.storage,
            &bws,
            &url_input,
            &confirmation,
        )
        .await
    }

    /// 入力した clone URL が domain rule で不正なとき、application の URL 検証で停止し、`create_*` /
    /// `update_*` のいずれにも進ませないことを検証する。
    ///
    /// 非秘匿化に伴い URL 形式検証は application（[`PasswordStoreRemote::parse`]）で行うため、不正 URL は
    /// port から取得した直後に停止し、BWS 保存境界へ到達しない。
    #[tokio::test]
    async fn provision_stops_when_input_url_is_invalid() {
        let mut yk = yubikey_access_mocks();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        // 同名 secret は未登録 → create 経路へ進む。
        bws.expect_list_bws_secrets()
            .returning(|_, _| Ok(Vec::new()));

        // 不在経路では確認なし。
        let mut confirmation = ports::MockBackupUpdateConfirmationPort::new();
        confirmation.expect_confirm_secret_overwrite().times(0);

        // port から不正 clone URL（domain 妥当でない値）を入力する。
        const INVALID_URL: &str = "https://example.invalid/repo.git";
        let mut url_input = ports::MockPasswordStoreRemoteInputPort::new();
        url_input
            .expect_read_password_store_remote_url()
            .times(1)
            .returning(|| Ok(INVALID_URL.to_owned()));

        // URL 検証で停止するため、保存境界へは到達しない。
        bws.expect_create_password_store_remote().times(0);
        bws.expect_update_password_store_remote_if_unchanged()
            .times(0);

        let result = run_provision_password_store_remote(
            command(false),
            &mut yk.device,
            &mut yk.storage,
            &bws,
            &url_input,
            &confirmation,
        )
        .await;

        assert!(
            result.is_err(),
            "invalid clone URL must stop provisioning before the BWS save boundary"
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
        let mut yk = yubikey_access_mocks();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![crate::domain::bws::BwsLookupCandidate {
                id: crate::domain::bws::BwsSecretId::new("pass-id"),
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

        let mut url_input = ports::MockPasswordStoreRemoteInputPort::new();
        url_input
            .expect_read_password_store_remote_url()
            .times(1)
            .returning(|| Ok(REMOTE_URL.to_owned()));

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
            &mut yk.device,
            &mut yk.storage,
            &bws,
            &url_input,
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
        let mut yk = yubikey_access_mocks();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![crate::domain::bws::BwsLookupCandidate {
                id: crate::domain::bws::BwsSecretId::new("pass-id"),
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
        let mut url_input = ports::MockPasswordStoreRemoteInputPort::new();
        url_input.expect_read_password_store_remote_url().times(0);

        let mut confirmation = ports::MockBackupUpdateConfirmationPort::new();
        confirmation
            .expect_confirm_secret_overwrite()
            .times(1)
            .returning(|_, _, _| Ok(false));

        let result = run_provision_password_store_remote(
            command(false),
            &mut yk.device,
            &mut yk.storage,
            &bws,
            &url_input,
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
        let mut yk = yubikey_access_mocks();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![
                crate::domain::bws::BwsLookupCandidate {
                    id: crate::domain::bws::BwsSecretId::new("dup-1"),
                    name: "password-store-remote".to_owned(),
                },
                crate::domain::bws::BwsLookupCandidate {
                    id: crate::domain::bws::BwsSecretId::new("dup-2"),
                    name: "password-store-remote".to_owned(),
                },
            ])
        });
        // 複数件検出で guard 取得・create・update のいずれにも進ませない。
        bws.expect_fetch_password_store_remote_guard().times(0);
        bws.expect_create_password_store_remote().times(0);
        bws.expect_update_password_store_remote_if_unchanged()
            .times(0);

        let mut url_input = ports::MockPasswordStoreRemoteInputPort::new();
        url_input.expect_read_password_store_remote_url().times(0);
        let mut confirmation = ports::MockBackupUpdateConfirmationPort::new();
        confirmation.expect_confirm_secret_overwrite().times(0);

        let result = run_provision_password_store_remote(
            command(true),
            &mut yk.device,
            &mut yk.storage,
            &bws,
            &url_input,
            &confirmation,
        )
        .await;

        assert!(
            result.is_err(),
            "duplicate password-store-remote secrets must stop provisioning"
        );
    }
}
