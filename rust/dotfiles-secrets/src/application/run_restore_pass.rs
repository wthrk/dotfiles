//! restore-pass の clone 順序を固定し、Git / SSH agent / filesystem の low-level 操作を port 境界の
//! 外へ閉じる。

use anyhow::Context;

use crate::Result;
use crate::{
    domain::{
        commands::RestorePassCommand,
        pass_restore::{PASSWORD_STORE_DIR_NAME, RestorePassSummary},
        piv::{SecretName, validate_piv_pin_len},
        storage::SecretStorageReadIntent,
        vault::{BitwardenAccountApiKey, BitwardenVaultCredentials, VaultSecretName},
    },
    ports,
};

/// `run_restore_pass` が使う外部 capability を named field で束ねる。
pub(crate) struct RestorePassRuntime<'a, B> {
    pub(crate) device: &'a mut dyn ports::yubikey::YubiKeyDevicePort,
    pub(crate) process: &'a dyn ports::io::PinInputPort,
    pub(crate) secret_input: &'a dyn ports::io::SecretInputPort,
    pub(crate) storage: &'a mut dyn ports::yubikey::SecretStoragePort,
    pub(crate) vault_client: &'a B,
    pub(crate) keyring: &'a mut dyn ports::gpg::GpgKeyringPort,
    pub(crate) store: &'a mut dyn ports::git::PasswordStorePort,
    pub(crate) git_clone: &'a mut dyn ports::git::GitClonePort,
    pub(crate) report: &'a dyn ports::io::ReportPort,
}

/// `password-store-remote` を取得し、`~/.password-store` 不存在を確認してから private repository を
/// SSH clone し、`pass` が store を読めることを確認する。
///
/// `secret-recovery-spec.md` が定義する、個人 vault からの `password-store-remote` 取得と
/// GPG authentication subkey 経由の SSH agent 認証 clone を application の順序制御として固定する。
/// この実装ではさらに、vault 認証材料取得 → `password-store-remote` 取得（URL 妥当性は domain 検証で確定）
/// → `~/.password-store` 不存在確認 → SSH clone → clone 後 store 可読性確認（サンプル entry の実復号を
/// 最終判定とし、空 store のみ recipient のいずれか 1 つの秘密鍵保持で代替）、という順序を固定する。
/// これは次の停止条件の責務境界を保護するためである。
///
/// gpg-agent SSH support が利用可能（socket 解決 + authentication subkey 識別可能）であることの確認は
/// `restore-gpg` の責務であり（`gnupg-ssh-design.md` の gpg-agent SSH support 経路）、
/// `restore-gpg` がその要件を満たさない場合に停止して
/// `restore-pass` へ進ませない。したがって `restore-pass` はその setup を信頼し、ssh-agent の identity を
/// 再検査せずに `git2 + SSH agent` 経路で clone する。
///
/// - clone 前に既存 store を破壊しない（不存在確認を先に止める）。
/// - clone 後 store が `pass` から読める（`.gpg-id` が非空で、サンプル entry が復元済み秘密鍵で復号できる。
///   空 store では recipient のいずれか 1 つの秘密鍵を保持する）まで完了とみなさない。複数 recipient や
///   email・user-id 形式の `.gpg-id` を誤って拒否しない。
/// - clone は adapter が `create_dir` で `~/.password-store` を原子的に確保してから clone し、失敗時は自分が
///   作成した destination を削除して残さない（既存 store は決して上書き・削除しない）。そのため clone 失敗時は
///   application 側で rollback せず error を伝播する（clone 前の不存在確認後に別 process が作った既存 store を
///   誤削除しないため）。clone 後の可読性確認で失敗した場合は、clone 済み store を application からは削除せず
///   そのまま残してエラーを返す。可読性失敗時の自動削除は clone 前の不存在確認後に別 process が
///   差し替えた store を誤削除しうる TOCTOU を持つ。再実行の安全性は既存 store 停止条件
///   （`~/.password-store` が既に存在する場合は停止）に委ね、再試行のため store は手動で削除させる。
///
/// 各停止条件で停止し、後続処理へ進ませない。clone URL / recipient / 可読性の業務判断は domain rule、clone /
/// filesystem 走査 / 鍵リング照合は adapter が担い、application は順序と停止条件だけを持つ。
pub(crate) async fn run_restore_pass<B>(
    command: RestorePassCommand,
    runtime: RestorePassRuntime<'_, B>,
) -> Result<()>
where
    B: ports::bw::VaultClientPort,
{
    let RestorePassRuntime {
        device,
        process,
        secret_input,
        storage: storage_port,
        vault_client,
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

    // 1. vault adapter が使う account API key を YubiKey storage から読み出す。
    let client_id_storage = SecretName::BitwardenClientId.storage_spec(serial);
    let client_id_inspection =
        storage_port.inspect_secret_storage_read(serial, &client_id_storage)?;
    let client_id_intent =
        SecretStorageReadIntent::from_inspection(client_id_storage, client_id_inspection)?;
    let client_id = storage_port
        .load_secret(serial, &client_id_intent, pin.as_ref())
        .map_err(|error| client_id_intent.decode_error(error))?;
    client_id_intent.validate_loaded_secret(&client_id)?;
    let client_secret_storage = SecretName::BitwardenClientSecret.storage_spec(serial);
    let client_secret_inspection =
        storage_port.inspect_secret_storage_read(serial, &client_secret_storage)?;
    let client_secret_intent =
        SecretStorageReadIntent::from_inspection(client_secret_storage, client_secret_inspection)?;
    let client_secret = storage_port
        .load_secret(serial, &client_secret_intent, pin.as_ref())
        .map_err(|error| client_secret_intent.decode_error(error))?;
    client_secret_intent.validate_loaded_secret(&client_secret)?;
    let master_password = secret_input.read_bitwarden_master_password()?;
    let credentials = BitwardenVaultCredentials::new(
        BitwardenAccountApiKey::new(client_id, client_secret),
        master_password,
    );

    // 2. 個人 vault から `password-store-remote` を取得する（URL 妥当性は domain 検証で確定済み）。
    let secret_id = VaultSecretName::PasswordStoreRemote
        .resolve_id(vault_client.list_vault_secrets(&credentials).await?)?;
    let remote = vault_client
        .fetch_password_store_remote(&credentials, &secret_id)
        .await?;

    // 3. `~/.password-store` が既に存在する場合は clone へ進まず停止する。
    if store.password_store_exists()? {
        anyhow::bail!("~/.password-store already exists; refusing to clone over it");
    }

    // 4. GPG authentication subkey 経由の SSH agent 認証で `~/.password-store` へ clone する。gpg-agent SSH support
    //    が利用可能（socket 解決 + authentication subkey 識別可能）であることは restore-gpg が確認済みであり
    //    （`gnupg-ssh-design.md` の gpg-agent SSH support 経路）、restore-pass はその setup を信頼して identity を再検査せずに clone する。clone は adapter が
    //    `create_dir` で `~/.password-store` を原子的に確保してから clone し、失敗時は自分が作成した destination を
    //    削除して残さない（既存 store は決して上書き・削除しない）。そのため clone 失敗時の application 側 rollback は
    //    行わず、error をそのまま伝播する（clone 前の不存在確認後に別 process が作った既存 store を誤削除しうる
    //    TOCTOU を避けるため）。
    git_clone.clone_password_store(&remote)?;

    // 5. clone 後 store が `pass` から実際に読めることを確認する。可読性確認で失敗した場合は clone 済み store を
    //    application からは削除せず、そのまま残してエラーを返す。可読性失敗時の自動削除は clone 前の
    //    不存在確認後に別 process が差し替えた store を誤削除しうる TOCTOU を持つ。再実行の安全性は
    //    既存 store 停止条件に委ね、再試行のため store は手動で削除させる。
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
        Err(error) => Err(error).with_context(|| {
            format!(
                "cloned ~/{PASSWORD_STORE_DIR_NAME} but could not read it with the available GPG key; the cloned store was left in place and must be removed manually before retrying"
            )
        }),
    }
}
