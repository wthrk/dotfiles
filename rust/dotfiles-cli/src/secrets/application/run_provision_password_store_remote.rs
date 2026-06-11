//! password-store-remote の保管側 provisioning 順序を固定し、値取得と検証を port/domain 境界へ閉じる。

use anyhow::Context;

use crate::Result;
use crate::secrets::{
    domain::{
        commands::ProvisionPasswordStoreRemoteCommand,
        pass_restore::PasswordStoreRemote,
        piv::{SecretName, validate_piv_pin_len},
        storage::SecretStorageReadIntent,
        vault::{
            BitwardenAccountApiKey, BitwardenVaultCredentials, VaultLookupResolution,
            VaultSecretName,
        },
    },
    ports,
};

/// `run_provision_password_store_remote` が使う外部 capability を named field で束ねる。
pub(crate) struct ProvisionPasswordStoreRemoteRuntime<'a, B> {
    pub(crate) device: &'a mut dyn ports::yubikey::DeviceSerialPort,
    pub(crate) pin_policy: &'a mut dyn ports::yubikey::DevicePinPolicyPort,
    pub(crate) pin_input: &'a dyn ports::io::PinInputPort,
    pub(crate) secret_input: &'a dyn ports::io::SecretInputPort,
    pub(crate) storage: &'a mut dyn ports::yubikey::SecretStoragePort,
    pub(crate) vault_client: &'a B,
    pub(crate) store: &'a dyn ports::git::PasswordStorePort,
    pub(crate) url_input: &'a dyn ports::io::PasswordStoreRemoteInputPort,
}

/// private `password-store` repository の clone URL を Bitwarden 個人 vault へ create または既存照合する
/// provisioning use case。
///
/// `secret-recovery-spec.md` と `bitwarden-personal-vault-design.md` が定める `password-store-remote` の
/// 保管コマンドを、`gpg-backup register`
/// と対称な順序制御として固定する。Bitwarden 個人 vault への接続には YubiKey storage に保存済みの
/// Bitwarden account API key `bitwarden-client-id` / `bitwarden-client-secret` と、CLI/app input port で
/// 取得した master password を使い、vault item name から vault item ID を解決（0件なら作成、1件なら使用、
/// 複数件なら停止）し、既存 `password-store-remote` secret の有無を確認する。不在なら input port から clone URL を取得して create し、ちょうど 1 件存在する場合は
/// configured origin から導ける権威的 repository identity と一致するときだけ既存値を使用する。
/// origin が無く照合できない既存値は fail-closed で停止する。
/// secret の複数件は domain failure として停止する。
/// account API key と clone URL は shell argv/stdin/env で中継せず、CLI/app 側の input port と
/// SDK/API adapter 境界で扱う。
///
/// 順序を application に固定するのは「vault 認証材料取得・vault item/secret の解決を済ませ、既存値がある場合は
/// 入力・保存へ進まない」停止条件の責務境界を保護するためである。clone URL は private repo の SSH clone URL であって秘密
/// 情報ではないため、保護 buffer・非表示入力・zeroize を使わず非秘匿の値として扱う。設定済み
/// password-store origin がある場合は repository identity として SSH/HTTPS GitHub URL を許容し、Bitwarden vault 登録値は
/// application/domain 側で `git@github.com:<owner>/<repo>.git` へ正規化する。origin が無い場合だけ
/// controlling TTY の可視対話入力から clone URL を取得し、`PasswordStoreRemote::parse` の URL 形式検証
/// （domain rule）へ通してから create へ渡す。既存値がある場合は provenance marker の後付け更新や
/// 値更新へ進まず、その Bitwarden vault secret を使用するか停止するかだけを決める。configured origin が観測できない
/// 既存値は権威的 identity と照合できないため停止する。
pub(crate) async fn run_provision_password_store_remote<B>(
    command: ProvisionPasswordStoreRemoteCommand,
    runtime: ProvisionPasswordStoreRemoteRuntime<'_, B>,
) -> Result<()>
where
    B: ports::bw::VaultClientPort,
{
    let ProvisionPasswordStoreRemoteRuntime {
        device,
        pin_policy,
        pin_input,
        secret_input,
        storage,
        vault_client,
        store,
        url_input,
    } = runtime;
    let _ = command;
    let serial = device.resolve_device_serial()?;
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
    .context("`pass-remote register` failed while reading Bitwarden vault credentials")?;

    // 既存 password-store-remote secret の候補を取得する。0件は create、1件は使用、複数件は停止。
    let secret_name = VaultSecretName::PasswordStoreRemote;
    let candidates = vault_client
        .list_vault_secrets(&credentials)
        .await
        .with_context(|| {
            format!(
                "`pass-remote register` failed while listing vault secret `{}`",
                secret_name.key()
            )
        })?;
    match secret_name.resolve_lookup(candidates) {
        VaultLookupResolution::Missing => {
            // 不在: clone URL を input port から取得し、検証してから新規 create する。
            let remote = match store.configured_origin_remote()? {
                Some(remote) => PasswordStoreRemote::from_configured_origin(&remote)?,
                None => PasswordStoreRemote::parse(&url_input.read_password_store_remote_url()?)?,
            };
            vault_client
                .create_password_store_remote(&credentials, &remote)
                .await
                .with_context(|| {
                    format!(
                        "`pass-remote register` failed while creating vault secret `{}`",
                        secret_name.key()
                    )
                })
                .map(|_id| ())
        }
        VaultLookupResolution::Unique(secret_id) => {
            let existing_remote = vault_client
                .fetch_password_store_remote(&credentials, &secret_id)
                .await
                .with_context(|| {
                    format!(
                        "`pass-remote register` failed while loading existing vault secret `{}`",
                        secret_name.key()
                    )
                })?;
            let Some(origin_remote) = store.configured_origin_remote()? else {
                anyhow::bail!(
                    "existing password-store-remote cannot be reused without a configured local origin"
                );
            };
            let expected_remote = PasswordStoreRemote::from_configured_origin(&origin_remote)?;
            if existing_remote != expected_remote {
                anyhow::bail!(
                    "existing password-store-remote does not match the configured local origin"
                );
            }
            Ok(())
        }
        VaultLookupResolution::Ambiguous => anyhow::bail!(
            "multiple password-store-remote secrets exist in the personal vault; refusing to provision"
        ),
    }
}

#[cfg(test)]
mod tests {
    use crate::secrets::{
        domain::{
            commands::ProvisionPasswordStoreRemoteCommand, manifest::SecretManifest,
            piv::SecretName, storage::SecretStorageReadInspection, vault::VaultSecretId,
        },
        ports,
        support::protection::ProtectedSecret,
    };

    use super::{ProvisionPasswordStoreRemoteRuntime, run_provision_password_store_remote};

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

    /// vault 操作の認証材料取得順序を固定し、master password を YubiKey storage へ保存しない。
    #[tokio::test]
    async fn pass_remote_register_reads_master_password_after_yubikey_api_key_without_storage_write()
    -> crate::Result<()> {
        let serial = 7001;
        let mut sequence = mockall::Sequence::new();
        let mut device = ports::yubikey::MockDeviceSerialPort::new();
        device
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
            .returning(|_| Ok(Vec::new()));
        let mut store = ports::git::MockPasswordStorePort::new();
        store
            .expect_configured_origin_remote()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(Some("git@github.com:owner/repo.git".to_owned())));
        vault_client
            .expect_create_password_store_remote()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(VaultSecretId::new("created")));
        vault_client.expect_fetch_gpg_backup_envelope().times(0);
        vault_client.expect_fetch_password_store_remote().times(0);
        store.expect_password_store_exists().times(0);
        store.expect_inspect_password_store().times(0);
        let mut url_input = ports::io::MockPasswordStoreRemoteInputPort::new();
        url_input.expect_read_password_store_remote_url().times(0);

        run_provision_password_store_remote(
            ProvisionPasswordStoreRemoteCommand,
            ProvisionPasswordStoreRemoteRuntime {
                device: &mut device,
                pin_policy: &mut pin_policy,
                pin_input: &pin_input,
                secret_input: &secret_input,
                storage: &mut storage,
                vault_client: &vault_client,
                store: &store,
                url_input: &url_input,
            },
        )
        .await?;

        Ok(())
    }
}
