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
/// 設計（spec L172-174、L92/L100/L210）の手順を順序制御として固定する。token 取得 →
/// `password-store-remote` 取得（URL 妥当性は domain 検証で確定）→ `~/.password-store` 不存在確認 →
/// **clone 前に gpg-agent SSH socket が復元した GPG authentication subkey の identity を提示していることを
/// 照合（#14 の key blob 照合）し、さらに `sshcontrol` が復元鍵の keygrip だけを持つことを確認** → GPG
/// authentication subkey 経由の SSH で clone → clone 後 store 可読性確認
/// （サンプル entry の実復号を最終判定とし、空 store のみ recipient のいずれか 1 つの秘密鍵保持で代替）→
/// 失敗時は clone 済み store をロールバック削除、という順序を application に固定するのは、次の停止条件の
/// 責務境界を保護するためである。
///
/// - clone 前に既存 store を破壊しない（不存在確認を先に止める）。
/// - 別 SSH key が repo access を持つ場合に restore-pass を成功させない。`Cred::ssh_key_from_agent` は agent 内
///   の任意 identity を提示しうるため、clone 前に期待 authentication subkey identity を agent が提示している
///   ことを `SshAgentReadiness::ensure_ready` で確定し、加えて `sshcontrol` が復元鍵の keygrip だけを持つ
///   ことを `SshControlRegistration::ensure_only` で確定してから clone へ進む（agent の単一鍵限定が API 上
///   不可能なための追加ガード。adapter 側は strict gpg-agent socket を併用）。
/// - clone 後 store が `pass` から読める（`.gpg-id` が非空で、サンプル entry が復元済み秘密鍵で復号できる。
///   空 store では recipient のいずれか 1 つの秘密鍵を保持する）まで完了とみなさない。複数 recipient や
///   email・user-id 形式の `.gpg-id` を誤って拒否しない。
/// - clone が `~/.password-store` を部分的に作成して失敗した場合、および clone 後の検証失敗時は、いずれも clone
///   済み `~/.password-store` を best-effort で削除し、次回実行が既存 store ガードで復旧不能にならないようにする
///   （restore-gpg の import 後 rollback と同じ原子化）。
///
/// 各停止条件で停止し、後続処理へ進ませない。clone URL / recipient / 可読性の業務判断は domain rule、clone /
/// filesystem 走査 / 鍵リング照合は adapter が担い、application は順序・停止条件・rollback だけを持つ。
#[expect(
    clippy::too_many_arguments,
    reason = "restore-pass は device/pin/storage/bws/keyring/ssh-agent/store/git-clone/report の port を順序適用する単一 use case"
)]
pub(crate) async fn run_restore_pass<D, P, S, B, K, A, G, C, R>(
    command: RestorePassCommand,
    device_serial: &mut D,
    pin_policy: &mut impl ports::DevicePinPolicyPort,
    process: &P,
    storage_port: &mut S,
    bws_client: &B,
    keyring: &mut K,
    ssh_agent: &mut A,
    store: &mut G,
    git_clone: &mut C,
    report: &R,
) -> Result<()>
where
    D: ports::DeviceSerialPort,
    P: ports::PinInputPort,
    S: ports::SecretStoragePort,
    B: ports::BwsClientPort,
    K: ports::GpgKeyringPort,
    A: ports::SshAgentPort,
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

    // 4. clone 前に、gpg-agent SSH socket が復元した GPG authentication subkey の identity を提示している
    //    ことを確認する。期待公開鍵は復元済み鍵リング（restore-gpg が import した recovery 鍵）の
    //    authentication subkey から取得し、agent が列挙する identity の key blob と byte 一致するかで照合する。
    //    別の SSH key だけが提示される場合はここで停止し、別鍵経由の clone 成功を防ぐ（spec L92/L100/L210）。
    let expected_public_key = keyring.resolve_recovery_authentication_ssh_public_key()?;
    ssh_agent
        .inspect_ssh_agent(&expected_public_key)?
        .ensure_ready()?;

    // 5. clone 直前に、gpg-agent の `sshcontrol` が復元鍵の keygrip だけを持つことを確認する。
    //    `Cred::ssh_key_from_agent` は agent が露出する任意 identity を提示しうるため、識別子照合（手順 4）だけ
    //    では別の登録済み identity 経由の clone を防げない。`sshcontrol` に復元鍵以外の authentication subkey が
    //    登録されている／復元鍵が未登録の場合はここで停止し、復元鍵以外の identity で clone が成立しないようにする
    //    （API 上 agent の単一鍵限定が不可能なための層境界内追加ガード。spec L92/L100/L210）。
    let recovery_keygrip = keyring.resolve_recovery_authentication_keygrip()?;
    ssh_agent
        .inspect_registered_keygrips()?
        .ensure_only(&recovery_keygrip)?;

    // 6. GPG authentication subkey 経由の SSH agent 認証で `~/.password-store` へ clone する。clone が store を
    //    部分的に作成して失敗した場合は、手順 7 の可読性確認失敗時と同様に clone 済み store を best-effort で削除
    //    （rollback）してから元エラーを返し、次回実行を既存 store ガードで復旧不能にしない。
    if let Err(error) = git_clone.clone_password_store(&remote) {
        let _ = store.remove_password_store();
        return Err(error);
    }

    // 7. clone 後 store が `pass` から実際に読めることを確認する。失敗した場合は clone 済み store を
    //    best-effort で削除（rollback）してから元エラーを返し、次回実行を既存 store ガードで止めない。
    match confirm_cloned_store_readable(keyring, store) {
        Ok(()) => report.write_restore_pass_report(&RestorePassSummary {
            store_path: format!("~/{PASSWORD_STORE_DIR_NAME}"),
            store_readable: true,
        }),
        Err(error) => {
            let _ = store.remove_password_store();
            Err(error)
        }
    }
}

/// clone 後 store が `pass` から読めることを確認する（実復号を最終判定とし、空 store のみ recipient 保持で代替）。
///
/// store 観測（`.gpg-id` 有無・recipient 行・サンプル entry）を取得し、domain で `.gpg-id` の存在・非空という
/// 構造的最小条件を確認する。サンプル entry があれば、それを復元済み秘密鍵で実際に復号できることが可読性の最終
/// 判定であり、recipient の数・形式（複数 recipient / email・user-id）には依存しない。entry が無い空 store では
/// 復号確認ができないため、`.gpg-id` recipient のうち少なくとも 1 つに対応する復元済み秘密鍵を保持していることを
/// 最小条件とする（全 recipient ではなく 1 つで可。`pass` は複数 recipient のいずれか 1 つの秘密鍵で復号できる）。
/// ここで返す失敗はすべて呼び出し側の rollback 削除対象になり、`pass` CLI への無条件シェルアウトはしない。
fn confirm_cloned_store_readable<K, G>(keyring: &mut K, store: &G) -> Result<()>
where
    K: ports::GpgKeyringPort,
    G: ports::PasswordStorePort,
{
    let readiness = store.inspect_password_store()?;
    let recipients = readiness.parse_recipients()?;
    match readiness.sample_entry() {
        Some(entry) => {
            // store に entry があれば、実際に復元済み秘密鍵で復号できることが可読性の最終判定。
            // recipient の数・形式（複数 recipient / email・user-id）に依存しない ground truth。
            keyring.can_decrypt_store_entry(entry)?;
        }
        None => {
            // entry が無い空 store では復号確認ができないため、`.gpg-id` recipient のうち
            // 少なくとも 1 つに対応する復元済み秘密鍵を保持していることを最小条件とする
            // （全 recipient ではなく 1 つで可。pass は複数 recipient のいずれか 1 つの秘密鍵で復号できる）。
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
    //! storage / BWS / keyring / ssh-agent / filesystem / git clone backend を port mock で差し替え、
    //! token 取得→remote 取得→`~/.password-store` 不存在確認→clone 前 identity 照合→clone→store 可読性確認
    //! （サンプル entry の実復号 / 空 store は recipient 1 つの秘密鍵保持）→失敗時 rollback という順序と、各停止
    //! 条件（既存 store / identity 不一致 / store 可読性不足）を検証する。test double は持ち込まない。

    use crate::secrets::{
        domain::{
            commands::RestorePassCommand,
            gpg_restore::{Keygrip, OpenSshPublicKey, SshAgentReadiness, SshControlRegistration},
            manifest::SecretManifest,
            pass_restore::{PasswordStoreReadiness, PasswordStoreRemote},
            storage::SecretStorageReadInspection,
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::run_restore_pass;

    const REMOTE_URL: &str = "git@github.com:owner/password-store.git";
    const RECOVERY_SSH_KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITESTBODY recovery";
    const RECIPIENT: &str = "0123456789ABCDEF0123456789ABCDEF01234567";
    const RECOVERY_KEYGRIP: &str = "0123456789ABCDEF0123456789ABCDEF01234567";
    const OTHER_KEYGRIP: &str = "FEDCBA9876543210FEDCBA9876543210FEDCBA98";

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

        // 不存在確認 → identity 照合 → clone → 可読性確認 の順序を Sequence で固定する。
        let mut store = ports::MockPasswordStorePort::new();
        store
            .expect_password_store_exists()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(false));
        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_resolve_recovery_authentication_ssh_public_key()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| OpenSshPublicKey::parse(RECOVERY_SSH_KEY));
        let mut ssh_agent = ports::MockSshAgentPort::new();
        ssh_agent
            .expect_inspect_ssh_agent()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|public_key| public_key.as_str() == RECOVERY_SSH_KEY)
            .returning(|_| {
                Ok(SshAgentReadiness {
                    socket_resolved: true,
                    authentication_identity_present: true,
                })
            });
        keyring
            .expect_resolve_recovery_authentication_keygrip()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Keygrip::parse(RECOVERY_KEYGRIP));
        ssh_agent
            .expect_inspect_registered_keygrips()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| {
                Ok(SshControlRegistration::new(vec![
                    Keygrip::parse(RECOVERY_KEYGRIP).expect("keygrip"),
                ]))
            });
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
            RestorePassCommand { serial: Some(2001) },
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &bws,
            &mut keyring,
            &mut ssh_agent,
            &mut store,
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
        // 既存 store では identity 照合・clone・可読性確認・rollback を行わない。
        store.expect_inspect_password_store().times(0);
        store.expect_remove_password_store().times(0);
        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_resolve_recovery_authentication_ssh_public_key()
            .times(0);
        let mut ssh_agent = ports::MockSshAgentPort::new();
        ssh_agent.expect_inspect_ssh_agent().times(0);
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
            &mut keyring,
            &mut ssh_agent,
            &mut store,
            &mut git_clone,
            &report,
        )
        .await;

        assert!(
            result.is_err(),
            "existing ~/.password-store must stop before clone"
        );
    }

    /// clone 前の identity 照合で、gpg-agent socket が復元 GPG authentication subkey identity を提示して
    /// いなければ clone へ進ませず停止することを検証する（別 SSH key 経由の clone 成功を防ぐ）。
    #[tokio::test]
    async fn restore_pass_stops_when_agent_identity_mismatches() {
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
        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_resolve_recovery_authentication_ssh_public_key()
            .times(1)
            .returning(|| OpenSshPublicKey::parse(RECOVERY_SSH_KEY));
        // agent は期待 identity を提示しない（authentication_identity_present = false）。
        let mut ssh_agent = ports::MockSshAgentPort::new();
        ssh_agent
            .expect_inspect_ssh_agent()
            .times(1)
            .returning(|_| {
                Ok(SshAgentReadiness {
                    socket_resolved: true,
                    authentication_identity_present: false,
                })
            });
        // identity 不一致では clone へ進ませず、rollback も不要。
        let mut git_clone = ports::MockGitClonePort::new();
        git_clone.expect_clone_password_store().times(0);
        store.expect_remove_password_store().times(0);
        let mut report = ports::MockReportPort::new();
        report.expect_write_restore_pass_report().times(0);

        let result = run_restore_pass(
            RestorePassCommand { serial: Some(2001) },
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &bws,
            &mut keyring,
            &mut ssh_agent,
            &mut store,
            &mut git_clone,
            &report,
        )
        .await;

        assert!(
            result.is_err(),
            "agent identity mismatch must stop before clone"
        );
    }

    /// clone 前の sshcontrol single-key 照合で、復元鍵以外の keygrip が gpg-agent に登録されている場合は
    /// clone へ進ませず停止することを検証する（別の登録済み identity 経由の clone 成立を防ぐ）。
    #[tokio::test]
    async fn restore_pass_stops_when_sshcontrol_has_other_keygrip() {
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
        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_resolve_recovery_authentication_ssh_public_key()
            .returning(|| OpenSshPublicKey::parse(RECOVERY_SSH_KEY));
        // identity 照合は通るが、sshcontrol に復元鍵以外の keygrip も登録されている。
        let mut ssh_agent = ports::MockSshAgentPort::new();
        ssh_agent.expect_inspect_ssh_agent().returning(|_| {
            Ok(SshAgentReadiness {
                socket_resolved: true,
                authentication_identity_present: true,
            })
        });
        keyring
            .expect_resolve_recovery_authentication_keygrip()
            .returning(|| Keygrip::parse(RECOVERY_KEYGRIP));
        ssh_agent
            .expect_inspect_registered_keygrips()
            .returning(|| {
                Ok(SshControlRegistration::new(vec![
                    Keygrip::parse(RECOVERY_KEYGRIP).expect("keygrip"),
                    Keygrip::parse(OTHER_KEYGRIP).expect("keygrip"),
                ]))
            });
        // 復元鍵以外の identity が登録されているため clone へ進ませず、rollback も不要。
        let mut git_clone = ports::MockGitClonePort::new();
        git_clone.expect_clone_password_store().times(0);
        store.expect_remove_password_store().times(0);
        let mut report = ports::MockReportPort::new();
        report.expect_write_restore_pass_report().times(0);

        let result = run_restore_pass(
            RestorePassCommand { serial: Some(2001) },
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &bws,
            &mut keyring,
            &mut ssh_agent,
            &mut store,
            &mut git_clone,
            &report,
        )
        .await;

        assert!(
            result.is_err(),
            "a non-recovery keygrip in sshcontrol must stop before clone"
        );
    }

    /// clone は成功したが clone 後の可読性確認（`.gpg-id` 不在）で失敗した場合、clone 済み store を
    /// `remove_password_store` で rollback 削除してから元エラーを返すことを検証する。
    #[tokio::test]
    async fn restore_pass_rolls_back_when_cloned_store_is_unreadable() {
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
        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_resolve_recovery_authentication_ssh_public_key()
            .returning(|| OpenSshPublicKey::parse(RECOVERY_SSH_KEY));
        let mut ssh_agent = ports::MockSshAgentPort::new();
        ssh_agent.expect_inspect_ssh_agent().returning(|_| {
            Ok(SshAgentReadiness {
                socket_resolved: true,
                authentication_identity_present: true,
            })
        });
        keyring
            .expect_resolve_recovery_authentication_keygrip()
            .returning(|| Keygrip::parse(RECOVERY_KEYGRIP));
        ssh_agent
            .expect_inspect_registered_keygrips()
            .returning(|| {
                Ok(SshControlRegistration::new(vec![
                    Keygrip::parse(RECOVERY_KEYGRIP).expect("keygrip"),
                ]))
            });
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
        // 検証失敗で clone 済み store を rollback 削除する。
        store
            .expect_remove_password_store()
            .times(1)
            .returning(|| Ok(()));
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
            &mut keyring,
            &mut ssh_agent,
            &mut store,
            &mut git_clone,
            &report,
        )
        .await;

        assert!(
            result.is_err(),
            "unreadable cloned store must fail restore-pass after rollback"
        );
    }

    /// clone が `~/.password-store` を部分的に作成して失敗した場合、clone 済み store を `remove_password_store`
    /// で rollback 削除してから元エラーを返し、clone 後の手順（可読性確認）へ進まないことを検証する。
    #[tokio::test]
    async fn restore_pass_rolls_back_when_clone_fails() {
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
        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_resolve_recovery_authentication_ssh_public_key()
            .returning(|| OpenSshPublicKey::parse(RECOVERY_SSH_KEY));
        let mut ssh_agent = ports::MockSshAgentPort::new();
        ssh_agent.expect_inspect_ssh_agent().returning(|_| {
            Ok(SshAgentReadiness {
                socket_resolved: true,
                authentication_identity_present: true,
            })
        });
        keyring
            .expect_resolve_recovery_authentication_keygrip()
            .returning(|| Keygrip::parse(RECOVERY_KEYGRIP));
        ssh_agent
            .expect_inspect_registered_keygrips()
            .returning(|| {
                Ok(SshControlRegistration::new(vec![
                    Keygrip::parse(RECOVERY_KEYGRIP).expect("keygrip"),
                ]))
            });
        // clone が store を部分的に作成して失敗する。
        let mut git_clone = ports::MockGitClonePort::new();
        git_clone
            .expect_clone_password_store()
            .times(1)
            .returning(|_| anyhow::bail!("network drop during clone left a partial store"));
        // clone 失敗で clone 済み（部分作成）store を rollback 削除する。
        store
            .expect_remove_password_store()
            .times(1)
            .returning(|| Ok(()));
        // clone 失敗後は clone 後の手順（可読性確認・実復号）へ進まない。
        store.expect_inspect_password_store().times(0);
        keyring.expect_can_decrypt_store_entry().times(0);
        keyring.expect_secret_key_available_for_recipient().times(0);
        let mut report = ports::MockReportPort::new();
        report.expect_write_restore_pass_report().times(0);

        let result = run_restore_pass(
            RestorePassCommand { serial: Some(2001) },
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &bws,
            &mut keyring,
            &mut ssh_agent,
            &mut store,
            &mut git_clone,
            &report,
        )
        .await;

        assert!(
            result.is_err(),
            "a clone failure that partially created the store must fail restore-pass after rollback"
        );
    }

    /// entry が無い空 store で、`.gpg-id` recipient のいずれにも対応する復元済み秘密鍵が無い場合、
    /// 可読性確認で停止し clone 済み store を rollback 削除することを検証する（空 store フォールバック）。
    #[tokio::test]
    async fn restore_pass_rolls_back_when_no_recipient_secret_key_for_empty_store() {
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
        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_resolve_recovery_authentication_ssh_public_key()
            .returning(|| OpenSshPublicKey::parse(RECOVERY_SSH_KEY));
        let mut ssh_agent = ports::MockSshAgentPort::new();
        ssh_agent.expect_inspect_ssh_agent().returning(|_| {
            Ok(SshAgentReadiness {
                socket_resolved: true,
                authentication_identity_present: true,
            })
        });
        keyring
            .expect_resolve_recovery_authentication_keygrip()
            .returning(|| Keygrip::parse(RECOVERY_KEYGRIP));
        ssh_agent
            .expect_inspect_registered_keygrips()
            .returning(|| {
                Ok(SshControlRegistration::new(vec![
                    Keygrip::parse(RECOVERY_KEYGRIP).expect("keygrip"),
                ]))
            });
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
        store
            .expect_remove_password_store()
            .times(1)
            .returning(|| Ok(()));
        let mut git_clone = ports::MockGitClonePort::new();
        git_clone
            .expect_clone_password_store()
            .times(1)
            .returning(|_| Ok(()));
        let mut report = ports::MockReportPort::new();
        report.expect_write_restore_pass_report().times(0);

        let result = run_restore_pass(
            RestorePassCommand { serial: Some(2001) },
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &bws,
            &mut keyring,
            &mut ssh_agent,
            &mut store,
            &mut git_clone,
            &report,
        )
        .await;

        assert!(
            result.is_err(),
            "empty store with no held recipient secret key must fail restore-pass after rollback"
        );
    }

    /// サンプル entry が存在するのに復号できない場合、recipient 保持の有無に関わらず可読性確認で停止し
    /// clone 済み store を rollback 削除することを検証する（実復号が最終判定であることの証明）。
    #[tokio::test]
    async fn restore_pass_rolls_back_when_sample_entry_cannot_decrypt() {
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
        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_resolve_recovery_authentication_ssh_public_key()
            .returning(|| OpenSshPublicKey::parse(RECOVERY_SSH_KEY));
        let mut ssh_agent = ports::MockSshAgentPort::new();
        ssh_agent.expect_inspect_ssh_agent().returning(|_| {
            Ok(SshAgentReadiness {
                socket_resolved: true,
                authentication_identity_present: true,
            })
        });
        keyring
            .expect_resolve_recovery_authentication_keygrip()
            .returning(|| Keygrip::parse(RECOVERY_KEYGRIP));
        ssh_agent
            .expect_inspect_registered_keygrips()
            .returning(|| {
                Ok(SshControlRegistration::new(vec![
                    Keygrip::parse(RECOVERY_KEYGRIP).expect("keygrip"),
                ]))
            });
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
        store
            .expect_remove_password_store()
            .times(1)
            .returning(|| Ok(()));
        let mut git_clone = ports::MockGitClonePort::new();
        git_clone
            .expect_clone_password_store()
            .times(1)
            .returning(|_| Ok(()));
        let mut report = ports::MockReportPort::new();
        report.expect_write_restore_pass_report().times(0);

        let result = run_restore_pass(
            RestorePassCommand { serial: Some(2001) },
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &bws,
            &mut keyring,
            &mut ssh_agent,
            &mut store,
            &mut git_clone,
            &report,
        )
        .await;

        assert!(
            result.is_err(),
            "a sample entry that cannot be decrypted must fail restore-pass after rollback"
        );
    }

    /// `.gpg-id` に複数 recipient があり全 recipient の秘密鍵は手元に無くても、サンプル entry が復号できれば
    /// restore-pass が成功することを検証する（全 recipient 保持を要求しないことの証明）。
    #[tokio::test]
    async fn restore_pass_succeeds_for_multi_recipient_store_when_entry_decrypts()
    -> crate::Result<()> {
        const SECOND_RECIPIENT: &str = "FEDCBA9876543210FEDCBA9876543210FEDCBA98";
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
        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_resolve_recovery_authentication_ssh_public_key()
            .returning(|| OpenSshPublicKey::parse(RECOVERY_SSH_KEY));
        let mut ssh_agent = ports::MockSshAgentPort::new();
        ssh_agent.expect_inspect_ssh_agent().returning(|_| {
            Ok(SshAgentReadiness {
                socket_resolved: true,
                authentication_identity_present: true,
            })
        });
        keyring
            .expect_resolve_recovery_authentication_keygrip()
            .returning(|| Keygrip::parse(RECOVERY_KEYGRIP));
        ssh_agent
            .expect_inspect_registered_keygrips()
            .returning(|| {
                Ok(SshControlRegistration::new(vec![
                    Keygrip::parse(RECOVERY_KEYGRIP).expect("keygrip"),
                ]))
            });
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
        store.expect_remove_password_store().times(0);
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
            RestorePassCommand { serial: Some(2001) },
            &mut device,
            &mut pin_policy,
            &process,
            &mut storage,
            &bws,
            &mut keyring,
            &mut ssh_agent,
            &mut store,
            &mut git_clone,
            &report,
        )
        .await
    }
}
