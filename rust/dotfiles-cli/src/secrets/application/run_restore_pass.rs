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

/// `run_restore_pass` が使う外部 capability を named field で束ねる。
pub(crate) struct RestorePassRuntime<'a, B> {
    pub(crate) device: &'a mut dyn ports::YubiKeyDevicePort,
    pub(crate) process: &'a dyn ports::PinInputPort,
    pub(crate) storage: &'a mut dyn ports::SecretStoragePort,
    pub(crate) bws_client: &'a B,
    pub(crate) keyring: &'a mut dyn ports::GpgKeyringPort,
    pub(crate) store: &'a mut dyn ports::PasswordStorePort,
    pub(crate) git_clone: &'a mut dyn ports::GitClonePort,
    pub(crate) report: &'a dyn ports::ReportPort,
}

/// `password-store-remote` を取得し、`~/.password-store` 不存在を確認してから private repository を
/// SSH clone し、`pass` が store を読めることを確認する。
///
/// 設計（spec L172-174）の手順を順序制御として固定する。token 取得 → `password-store-remote` 取得（URL
/// 妥当性は domain 検証で確定）→ `~/.password-store` 不存在確認 → GPG authentication subkey 経由の SSH で
/// clone → clone 後 store 可読性確認
/// （サンプル entry の実復号を最終判定とし、空 store のみ recipient のいずれか 1 つの秘密鍵保持で代替）、
/// という順序を application に固定するのは、次の停止条件の責務境界を保護するためである。
///
/// gpg-agent SSH support が利用可能（socket 解決 + authentication subkey 識別可能）であることの確認は
/// `restore-gpg` の責務であり（設計 L116-124）、`restore-gpg` がその要件を満たさない場合に停止して
/// `restore-pass` へ進ませない。したがって `restore-pass` はその setup を信頼し、ssh-agent の identity を
/// 再検査せずに `git2 + SSH agent` 経路で clone する。
///
/// - clone 前に既存 store を破壊しない（不存在確認を先に止める）。
/// - clone 後 store が `pass` から読める（`.gpg-id` が非空で、サンプル entry が復元済み秘密鍵で復号できる。
///   空 store では recipient のいずれか 1 つの秘密鍵を保持する）まで完了とみなさない。複数 recipient や
///   email・user-id 形式の `.gpg-id` を誤って拒否しない。
/// - clone は adapter が `create_dir` で `~/.password-store` を原子的に確保してから clone し、失敗時は自分が
///   作成した destination を削除して残さない（既存 store は決して上書き・削除しない）。そのため clone 失敗時は
///   application 側で rollback せず error を伝播する（手順 3 の不存在確認後に別 process が作った既存 store を
///   誤削除しないため）。clone 後の可読性確認で失敗した場合は、clone 済み store を application からは削除せず
///   そのまま残してエラーを返す。設計（spec L174）に可読性失敗時の自動削除は無く、削除は手順 3 の不存在確認後に
///   別 process が差し替えた store を誤削除しうる TOCTOU を持つ。再実行の安全性は既存 store 停止条件
///   （spec L212: `~/.password-store` が既に存在する → 停止）に委ね、再試行のため store は手動で削除させる。
///
/// 各停止条件で停止し、後続処理へ進ませない。clone URL / recipient / 可読性の業務判断は domain rule、clone /
/// filesystem 走査 / 鍵リング照合は adapter が担い、application は順序と停止条件だけを持つ。
pub(crate) async fn run_restore_pass<B>(
    command: RestorePassCommand,
    runtime: RestorePassRuntime<'_, B>,
) -> Result<()>
where
    B: ports::BwsClientPort,
{
    let RestorePassRuntime {
        device,
        process,
        storage: storage_port,
        bws_client,
        keyring,
        store,
        git_clone,
        report,
    } = runtime;
    let _ = command;
    let serial = device.resolve_device_serial()?;
    let pin = if device.device_requires_pin(serial)? {
        let pin = process.read_pin()?;
        validate_piv_pin_len(pin.len())?;
        Some(pin)
    } else {
        None
    };

    // 1. bws-access-token を YubiKey storage から読み出す。
    let storage = SecretName::BwsAccessToken.storage_spec(serial);
    let inspection = storage_port.inspect_secret_storage_read(serial, &storage)?;
    let intent = SecretStorageReadIntent::from_inspection(storage, inspection)?;
    let access_token = storage_port
        .load_secret(serial, &intent, pin.as_ref())
        .map_err(|error| intent.decode_error(error))?;
    intent.validate_loaded_secret(&access_token)?;

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

    // 4. GPG authentication subkey 経由の SSH agent 認証で `~/.password-store` へ clone する。gpg-agent SSH support
    //    が利用可能（socket 解決 + authentication subkey 識別可能）であることは restore-gpg が確認済みであり
    //    （設計 L116-124）、restore-pass はその setup を信頼して identity を再検査せずに clone する。clone は adapter が
    //    `create_dir` で `~/.password-store` を原子的に確保してから clone し、失敗時は自分が作成した destination を
    //    削除して残さない（既存 store は決して上書き・削除しない）。そのため clone 失敗時の application 側 rollback は
    //    行わず、error をそのまま伝播する（手順 3 の不存在確認後に別 process が作った既存 store を誤削除しうる
    //    TOCTOU を避けるため）。
    git_clone.clone_password_store(&remote)?;

    // 5. clone 後 store が `pass` から実際に読めることを確認する。可読性確認で失敗した場合は clone 済み store を
    //    application からは削除せず、そのまま残してエラーを返す。設計（spec L174）に可読性失敗時の自動削除は無く、
    //    削除は手順 3 の不存在確認後に別 process が差し替えた store を誤削除しうる TOCTOU を持つ。再実行の安全性は
    //    既存 store 停止条件（手順 3 / spec L212）に委ね、再試行のため store は手動で削除させる。
    let store_readability = (|| -> Result<()> {
        let readiness = store.inspect_password_store()?;
        let recipients = readiness.parse_recipients()?;
        match readiness.sample_entry() {
            Some(entry) => {
                keyring.can_decrypt_store_entry(entry)?;
            }
            None => {
                let mut any_available = false;
                for recipient in &recipients {
                    if keyring.secret_key_available_for_recipient(recipient)? {
                        any_available = true;
                        break;
                    }
                }
                if !any_available {
                    anyhow::bail!(
                        "cloned password-store is encrypted only to GPG keys whose secret keys are not in the keyring; pass cannot decrypt it"
                    );
                }
            }
        }
        Ok(())
    })();
    match store_readability {
        Ok(()) => report.write_restore_pass_report(&RestorePassSummary {
            store_path: format!("~/{PASSWORD_STORE_DIR_NAME}"),
            store_readable: true,
        }),
        Err(error) => Err(anyhow::anyhow!(
            "cloned ~/{PASSWORD_STORE_DIR_NAME} but could not read it with the available GPG key ({error:#}); the cloned store was left in place and must be removed manually before retrying"
        )),
    }
}

#[cfg(test)]
mod tests {
    //! restore-pass の順序制御と停止条件を mockall + Sequence で検証する単体テスト。
    //!
    //! storage / BWS / keyring / filesystem / git clone backend を port mock で差し替え、
    //! token 取得→remote 取得→`~/.password-store` 不存在確認→clone→store
    //! 可読性確認（サンプル entry の実復号 / 空 store は recipient 1 つの秘密鍵保持）
    //! という順序と、各停止条件（既存 store / store 可読性不足 / clone 失敗）で停止することを検証する。
    //! 可読性確認失敗時も clone 失敗時も application は store を削除せず error を伝播し、再実行の安全性は
    //! 既存 store 停止条件に委ねる（設計 spec L174 に自動削除は無い）。gpg-agent SSH support の確認は
    //! restore-gpg の責務であり restore-pass は ssh-agent を検査しない（設計 L116-124）。test double は持ち込まない。

    use crate::secrets::{
        domain::{
            commands::RestorePassCommand,
            manifest::SecretManifest,
            pass_restore::{PasswordStoreReadiness, PasswordStoreRemote},
            storage::SecretStorageReadInspection,
        },
        ports,
        ports::ProtectedSecret,
    };

    use super::{RestorePassRuntime, run_restore_pass};

    const REMOTE_URL: &str = "git@github.com:owner/password-store.git";
    const RECIPIENT: &str = "0123456789ABCDEF0123456789ABCDEF01234567";

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn read_inspection() -> SecretStorageReadInspection {
        SecretStorageReadInspection {
            manifest_bytes: Some(SecretManifest::expected().encode().expect("manifest")),
            encoded: Some(vec![1]),
        }
    }

    /// recipient 1 件・サンプル entry 1 件を持つ「読める」store 観測値。
    fn readable_readiness() -> PasswordStoreReadiness {
        PasswordStoreReadiness {
            gpg_id_present: true,
            gpg_id_recipients: vec![RECIPIENT.to_owned()],
            sample_entry: Some(std::path::PathBuf::from("/store/sample.gpg")),
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
            .returning(|| Ok(2001));
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
        let mut keyring = ports::MockGpgKeyringPort::new();
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
            .returning(|| Ok(readable_readiness()));
        // サンプル entry があるので、可読性は実復号で判定する（recipient 保持確認は呼ばない）。
        keyring.expect_secret_key_available_for_recipient().times(0);
        keyring
            .expect_can_decrypt_store_entry()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(()));
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
            RestorePassCommand,
            RestorePassRuntime {
                device: &mut (&mut device, &mut pin_policy),
                process: &process,
                storage: &mut storage,
                bws_client: &bws,
                keyring: &mut keyring,
                store: &mut store,
                git_clone: &mut git_clone,
                report: &report,
            },
        )
        .await
    }

    #[tokio::test]
    async fn restore_pass_stops_when_store_already_exists() {
        let mut device = ports::MockDeviceSerialPort::new();
        device.expect_resolve_device_serial().returning(|| Ok(2001));
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
        // 既存 store では clone・可読性確認を行わない。
        store.expect_inspect_password_store().times(0);
        let mut keyring = ports::MockGpgKeyringPort::new();
        let mut git_clone = ports::MockGitClonePort::new();
        git_clone.expect_clone_password_store().times(0);
        let mut report = ports::MockReportPort::new();
        report.expect_write_restore_pass_report().times(0);

        let result = run_restore_pass(
            RestorePassCommand,
            RestorePassRuntime {
                device: &mut (&mut device, &mut pin_policy),
                process: &process,
                storage: &mut storage,
                bws_client: &bws,
                keyring: &mut keyring,
                store: &mut store,
                git_clone: &mut git_clone,
                report: &report,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "existing ~/.password-store must stop before clone"
        );
    }

    /// clone は成功したが clone 後の可読性確認（`.gpg-id` 不在）で失敗した場合、clone 済み store を削除せず
    /// （application からは store を消さない）元エラーを伝播することを検証する。再実行の安全性は既存 store
    /// 停止条件に委ね、store は手動削除させる。
    #[tokio::test]
    async fn restore_pass_errors_when_cloned_store_is_unreadable() {
        let mut device = ports::MockDeviceSerialPort::new();
        device.expect_resolve_device_serial().returning(|| Ok(2001));
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
        let mut keyring = ports::MockGpgKeyringPort::new();
        // clone は成功するが、store に `.gpg-id` がなく可読性確認で失敗する。
        store
            .expect_inspect_password_store()
            .times(1)
            .returning(|| {
                Ok(PasswordStoreReadiness {
                    gpg_id_present: false,
                    gpg_id_recipients: Vec::new(),
                    sample_entry: None,
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
            RestorePassCommand,
            RestorePassRuntime {
                device: &mut (&mut device, &mut pin_policy),
                process: &process,
                storage: &mut storage,
                bws_client: &bws,
                keyring: &mut keyring,
                store: &mut store,
                git_clone: &mut git_clone,
                report: &report,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "unreadable cloned store must fail restore-pass without deleting the store"
        );
    }

    /// clone が失敗した場合、application 側では store 削除を行わず（自分が `create_dir` で作成した destination を
    /// 掃除するのは adapter の責務）、元エラーを返して clone 後の手順（可読性確認）へ進まないことを検証する。手順 3 の
    /// 不存在確認後に別 process が作った既存 store を誤削除しないため、application は store を削除しない。
    #[tokio::test]
    async fn restore_pass_returns_error_without_rollback_when_clone_fails() {
        let mut device = ports::MockDeviceSerialPort::new();
        device.expect_resolve_device_serial().returning(|| Ok(2001));
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
        let mut keyring = ports::MockGpgKeyringPort::new();
        // clone が失敗する（adapter が自分の作った destination を掃除し残さない前提）。
        let mut git_clone = ports::MockGitClonePort::new();
        git_clone
            .expect_clone_password_store()
            .times(1)
            .returning(|_| anyhow::bail!("network drop during clone"));
        // clone 失敗時 application は store を削除しない（adapter が自分の作った destination を掃除する。
        // ここで削除すると TOCTOU で別 process の既存 store を誤削除しうる）。
        // clone 失敗後は clone 後の手順（可読性確認・実復号）へ進まない。
        store.expect_inspect_password_store().times(0);
        keyring.expect_can_decrypt_store_entry().times(0);
        keyring.expect_secret_key_available_for_recipient().times(0);
        let mut report = ports::MockReportPort::new();
        report.expect_write_restore_pass_report().times(0);

        let result = run_restore_pass(
            RestorePassCommand,
            RestorePassRuntime {
                device: &mut (&mut device, &mut pin_policy),
                process: &process,
                storage: &mut storage,
                bws_client: &bws,
                keyring: &mut keyring,
                store: &mut store,
                git_clone: &mut git_clone,
                report: &report,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "a clone failure must fail restore-pass without an application-level rollback"
        );
    }

    /// entry が無い空 store で、`.gpg-id` recipient のいずれにも対応する復元済み秘密鍵が無い場合、
    /// 可読性確認で停止し error を伝播すること（clone 済み store は削除しない）を検証する（空 store フォールバック）。
    #[tokio::test]
    async fn restore_pass_errors_when_no_recipient_secret_key_for_empty_store() {
        let mut device = ports::MockDeviceSerialPort::new();
        device.expect_resolve_device_serial().returning(|| Ok(2001));
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
        let mut keyring = ports::MockGpgKeyringPort::new();
        // entry が無い空 store。フォールバックで recipient 1 つの秘密鍵保持を確認する。
        store
            .expect_inspect_password_store()
            .times(1)
            .returning(|| {
                Ok(PasswordStoreReadiness {
                    gpg_id_present: true,
                    gpg_id_recipients: vec![RECIPIENT.to_owned()],
                    sample_entry: None,
                })
            });
        // recipient は妥当だが秘密鍵を保持しない → 空 store なので可読性を確定できず停止。
        keyring
            .expect_secret_key_available_for_recipient()
            .times(1)
            .returning(|_| Ok(false));
        // 空 store では復号確認は行わない。
        keyring.expect_can_decrypt_store_entry().times(0);
        let mut git_clone = ports::MockGitClonePort::new();
        git_clone
            .expect_clone_password_store()
            .times(1)
            .returning(|_| Ok(()));
        let mut report = ports::MockReportPort::new();
        report.expect_write_restore_pass_report().times(0);

        let result = run_restore_pass(
            RestorePassCommand,
            RestorePassRuntime {
                device: &mut (&mut device, &mut pin_policy),
                process: &process,
                storage: &mut storage,
                bws_client: &bws,
                keyring: &mut keyring,
                store: &mut store,
                git_clone: &mut git_clone,
                report: &report,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "empty store with no held recipient secret key must fail restore-pass without deleting the store"
        );
    }

    /// サンプル entry が存在するのに復号できない場合、recipient 保持の有無に関わらず可読性確認で停止し
    /// error を伝播すること（clone 済み store は削除しない）を検証する（実復号が最終判定であることの証明）。
    #[tokio::test]
    async fn restore_pass_errors_when_sample_entry_cannot_decrypt() {
        let mut device = ports::MockDeviceSerialPort::new();
        device.expect_resolve_device_serial().returning(|| Ok(2001));
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
        let mut keyring = ports::MockGpgKeyringPort::new();
        store
            .expect_inspect_password_store()
            .times(1)
            .returning(|| Ok(readable_readiness()));
        // サンプル entry があれば実復号が最終判定。recipient 保持確認は呼ばない。
        keyring.expect_secret_key_available_for_recipient().times(0);
        keyring
            .expect_can_decrypt_store_entry()
            .times(1)
            .returning(|_| anyhow::bail!("entry cannot be decrypted with restored key"));
        let mut git_clone = ports::MockGitClonePort::new();
        git_clone
            .expect_clone_password_store()
            .times(1)
            .returning(|_| Ok(()));
        let mut report = ports::MockReportPort::new();
        report.expect_write_restore_pass_report().times(0);

        let result = run_restore_pass(
            RestorePassCommand,
            RestorePassRuntime {
                device: &mut (&mut device, &mut pin_policy),
                process: &process,
                storage: &mut storage,
                bws_client: &bws,
                keyring: &mut keyring,
                store: &mut store,
                git_clone: &mut git_clone,
                report: &report,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "a sample entry that cannot be decrypted must fail restore-pass without deleting the store"
        );
    }

    /// `.gpg-id` に複数 recipient があり全 recipient の秘密鍵は手元に無くても、サンプル entry が復号できれば
    /// restore-pass が成功することを検証する（全 recipient 保持を要求しないことの証明）。
    #[tokio::test]
    async fn restore_pass_succeeds_for_multi_recipient_store_when_entry_decrypts()
    -> crate::Result<()> {
        const SECOND_RECIPIENT: &str = "FEDCBA9876543210FEDCBA9876543210FEDCBA98";
        let mut device = ports::MockDeviceSerialPort::new();
        device.expect_resolve_device_serial().returning(|| Ok(2001));
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
        let mut keyring = ports::MockGpgKeyringPort::new();
        // 2 recipient（spare/共有 store）。サンプル entry が復号できれば全 recipient 保持は不要。
        store
            .expect_inspect_password_store()
            .times(1)
            .returning(|| {
                Ok(PasswordStoreReadiness {
                    gpg_id_present: true,
                    gpg_id_recipients: vec![RECIPIENT.to_owned(), SECOND_RECIPIENT.to_owned()],
                    sample_entry: Some(std::path::PathBuf::from("/store/sample.gpg")),
                })
            });
        keyring.expect_secret_key_available_for_recipient().times(0);
        keyring
            .expect_can_decrypt_store_entry()
            .times(1)
            .returning(|_| Ok(()));
        let mut git_clone = ports::MockGitClonePort::new();
        git_clone
            .expect_clone_password_store()
            .times(1)
            .returning(|_| Ok(()));
        let mut report = ports::MockReportPort::new();
        report
            .expect_write_restore_pass_report()
            .times(1)
            .withf(|summary| summary.store_readable)
            .returning(|_| Ok(()));

        run_restore_pass(
            RestorePassCommand,
            RestorePassRuntime {
                device: &mut (&mut device, &mut pin_policy),
                process: &process,
                storage: &mut storage,
                bws_client: &bws,
                keyring: &mut keyring,
                store: &mut store,
                git_clone: &mut git_clone,
                report: &report,
            },
        )
        .await
    }
}
