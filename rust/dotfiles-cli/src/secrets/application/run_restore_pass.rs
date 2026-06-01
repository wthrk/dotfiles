//! restore-pass の clone 順序を固定し、Git / SSH agent / filesystem の low-level 操作を port 境界の
//! 外へ閉じる。

use crate::Result;
use crate::secrets::{
    domain::{
        bws::{BwsProjectName, BwsSecretName},
        commands::RestorePassCommand,
        pass_restore::{PASSWORD_STORE_DIR_NAME, RestorePassSummary},
        piv::{SecretName, validate_piv_pin_len},
        storage::SecretStorageReadIntent,
    },
    ports,
};

/// `password-store-remote` を取得し、`~/.password-store` 不存在を確認してから private repository を
/// SSH clone し、`pass` が store を読めることを確認する。
///
/// 設計（spec L172-174）の手順を順序制御として固定する。token 取得 → `password-store-remote` 取得
/// （URL 妥当性は domain 検証で確定）→ `~/.password-store` 不存在確認 → GPG authentication subkey 経由の
/// SSH で clone → clone 後 store 可読性確認、という順序を application に固定するのは、「clone 前に既存
/// store を破壊しない（不存在確認を先に止める）」「clone 後 store が `pass` から読めるまで完了とみなさない」
/// という停止条件の責務境界を保護するためである。各停止条件で停止し、後続処理へ進ませない。clone URL の
/// 妥当性や store 可読性の業務判断は domain rule、clone / filesystem 走査は adapter が担い、application は
/// 順序と停止条件だけを持つ。
#[expect(
    clippy::too_many_arguments,
    reason = "restore-pass は device/pin/storage/bws/store/git-clone/report の port を順序適用する単一 use case"
)]
pub(crate) async fn run_restore_pass<D, P, S, B, G, C, R>(
    command: RestorePassCommand,
    device_serial: &mut D,
    pin_policy: &mut impl ports::DevicePinPolicyPort,
    process: &P,
    storage_port: &mut S,
    bws_client: &B,
    store: &G,
    git_clone: &mut C,
    report: &R,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    P: ports::PinInputPort,
    S: ports::SecretStoragePort,
    B: ports::BwsClientPort,
    G: ports::PasswordStorePort,
    C: ports::GitClonePort,
    R: ports::ReportPort,
{
    let serial = device_serial.resolve_device_serial(command.serial)?;
    let pin = if pin_policy.device_requires_pin(serial)? {
        let pin = process.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };

    // 1. bws-access-token を YubiKey storage から読み出す。
    let access_token = load_bws_access_token(serial, storage_port, pin.as_ref())?;

    // 2. BWS から `password-store-remote` を取得する（URL 妥当性は domain 検証で確定済み）。
    let project_id = BwsProjectName::DOTFILES_SECRET_RECOVERY
        .resolve_id(bws_client.list_bws_projects(&access_token).await?)?;
    let secret_id = BwsSecretName::PasswordStoreRemote.resolve_id(
        bws_client
            .list_bws_secrets(&access_token, &project_id)
            .await?,
        &project_id,
    )?;
    let remote = bws_client
        .fetch_password_store_remote(&access_token, &secret_id)
        .await?;

    // 3. `~/.password-store` が既に存在する場合は clone へ進まず停止する。
    if store.password_store_exists()? {
        anyhow::bail!("~/.password-store already exists; refusing to clone over it");
    }

    // 4. GPG authentication subkey 経由の SSH agent 認証で `~/.password-store` へ clone する。
    git_clone.clone_password_store(&remote)?;

    // 5. clone 後 store が `pass` から読める構成（`.gpg-id` 存在）であることを確認する。
    store.inspect_password_store()?.ensure_readable()?;

    report.write_restore_pass_report(&RestorePassSummary {
        store_path: format!("~/{PASSWORD_STORE_DIR_NAME}"),
        store_readable: true,
    })
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
    //! restore-pass の順序制御と停止条件を mockall + Sequence で検証する単体テスト。
    //!
    //! storage / BWS / filesystem / git clone backend を port mock で差し替え、token 取得→remote 取得→
    //! `~/.password-store` 不存在確認→clone→store 可読性確認という順序と、各停止条件（既存 store / store
    //! 可読性不足）を検証する。test double は持ち込まない。

    use crate::secrets::{
        domain::{
            commands::RestorePassCommand,
            manifest::SecretManifest,
            pass_restore::{PasswordStoreReadiness, PasswordStoreRemote},
            storage::SecretStorageReadInspection,
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_restore_pass;

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

    fn expect_local_storage_ok(
        storage: &mut ports::MockSecretStoragePort,
        sequence: &mut mockall::Sequence,
    ) {
        storage
            .expect_inspect_secret_storage_read()
            .times(1)
            .in_sequence(sequence)
            .returning(|_, _| Ok(read_inspection()));
        storage
            .expect_load_secret()
            .times(1)
            .in_sequence(sequence)
            .returning(|_, _, _| Ok(material(b"access-token")));
    }

    fn expect_bws_remote_ok(bws: &mut ports::MockBwsClientPort) {
        bws.expect_list_bws_projects().times(1).returning(|_| {
            Ok(vec![crate::secrets::domain::bws::BwsLookupCandidate {
                id: crate::secrets::domain::bws::BwsProjectId::new("project-1"),
                name: "dotfiles-secret-recovery".to_owned(),
            }])
        });
        bws.expect_list_bws_secrets().times(1).returning(|_, _| {
            Ok(vec![crate::secrets::domain::bws::BwsLookupCandidate {
                id: crate::secrets::domain::bws::BwsSecretId::new("pass-id"),
                name: "password-store-remote".to_owned(),
            }])
        });
        bws.expect_fetch_password_store_remote()
            .times(1)
            .returning(|_, _| PasswordStoreRemote::parse(REMOTE_URL));
    }

    #[tokio::test]
    async fn restore_pass_runs_full_order() -> crate::Result<()> {
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
            .in_sequence(&mut sequence)
            .returning(|_| Ok(false));
        let process = ports::MockPinInputPort::new();
        let mut storage = ports::MockSecretStoragePort::new();
        expect_local_storage_ok(&mut storage, &mut sequence);

        let mut bws = ports::MockBwsClientPort::new();
        expect_bws_remote_ok(&mut bws);

        // 不存在確認 → clone → 可読性確認 の順序を Sequence で固定する。
        let mut store = ports::MockPasswordStorePort::new();
        store
            .expect_password_store_exists()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(false));
        let mut git_clone = ports::MockGitClonePort::new();
        git_clone
            .expect_clone_password_store()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|remote| remote.as_str() == REMOTE_URL)
            .returning(|_| Ok(()));
        store
            .expect_inspect_password_store()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| {
                Ok(PasswordStoreReadiness {
                    gpg_id_present: true,
                })
            });
        let mut report = ports::MockReportPort::new();
        report
            .expect_write_restore_pass_report()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|summary| {
                summary.store_readable && summary.store_path.ends_with(".password-store")
            })
            .returning(|_| Ok(()));

        run_restore_pass(
            RestorePassCommand { serial: Some(2001) },
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &bws,
            &store,
            &mut git_clone,
            &report,
        )
        .await
    }

    #[tokio::test]
    async fn restore_pass_stops_when_store_already_exists() {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
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
        expect_bws_remote_ok(&mut bws);

        let mut store = ports::MockPasswordStorePort::new();
        store.expect_password_store_exists().returning(|| Ok(true));
        // 既存 store では clone も可読性確認も行わない。
        store.expect_inspect_password_store().times(0);
        let mut git_clone = ports::MockGitClonePort::new();
        git_clone.expect_clone_password_store().times(0);
        let mut report = ports::MockReportPort::new();
        report.expect_write_restore_pass_report().times(0);

        let result = run_restore_pass(
            RestorePassCommand { serial: Some(2001) },
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &bws,
            &store,
            &mut git_clone,
            &report,
        )
        .await;

        assert!(
            result.is_err(),
            "existing ~/.password-store must stop before clone"
        );
    }

    #[tokio::test]
    async fn restore_pass_fails_when_cloned_store_is_unreadable() {
        let mut device = ports::MockDeviceSerialPort::new();
        device
            .expect_resolve_device_serial()
            .returning(|_| Ok(2001));
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
        expect_bws_remote_ok(&mut bws);

        let mut store = ports::MockPasswordStorePort::new();
        store.expect_password_store_exists().returning(|| Ok(false));
        // clone は成功するが、store に `.gpg-id` がなく可読性確認で失敗する。
        store
            .expect_inspect_password_store()
            .times(1)
            .returning(|| {
                Ok(PasswordStoreReadiness {
                    gpg_id_present: false,
                })
            });
        let mut git_clone = ports::MockGitClonePort::new();
        git_clone
            .expect_clone_password_store()
            .times(1)
            .returning(|_| Ok(()));
        let mut report = ports::MockReportPort::new();
        // 可読性確認で停止するため report は書かない。
        report.expect_write_restore_pass_report().times(0);

        let result = run_restore_pass(
            RestorePassCommand { serial: Some(2001) },
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &bws,
            &store,
            &mut git_clone,
            &report,
        )
        .await;

        assert!(
            result.is_err(),
            "unreadable cloned store must fail restore-pass"
        );
    }
}
