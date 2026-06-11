//! restore-gpg の鍵リング復元順序を固定し、GPG/SSH の low-level 操作を port 境界の外へ閉じる。

use crate::Result;
use crate::secrets::{
    domain::{commands::RestoreGpgCommand, gpg_restore::RestoreGpgSummary, vault::VaultSecretName},
    domain::{
        piv::{SecretName, validate_piv_pin_len},
        storage::SecretStorageReadIntent,
        vault::{BitwardenAccountApiKey, BitwardenVaultCredentials},
    },
    ports,
};

/// `run_restore_gpg` が使う外部 capability を named field で束ねる。
pub(crate) struct RestoreGpgRuntime<'a, B> {
    pub(crate) device: &'a mut dyn ports::yubikey::YubiKeyDevicePort,
    pub(crate) process: &'a dyn ports::io::PinInputPort,
    pub(crate) secret_input: &'a dyn ports::io::SecretInputPort,
    pub(crate) storage: &'a mut dyn ports::yubikey::SecretStoragePort,
    pub(crate) vault_client: &'a B,
    pub(crate) recipient: &'a mut dyn ports::yubikey::GpgRecipientPort,
    pub(crate) cipher: &'a mut dyn ports::gpg::BackupCipherPort,
    pub(crate) keyring: &'a mut dyn ports::gpg::GpgKeyringPort,
    pub(crate) ssh_agent: &'a mut dyn ports::gpg::SshAgentPort,
    pub(crate) report: &'a dyn ports::io::ReportPort,
}

/// `gpg-secret-key-backup` encrypted envelope を接続中 YubiKey で復号して鍵リングへ復元する。
///
/// `gnupg-ssh-design.md` が述べる encrypted envelope 復号、primary fingerprint 照合、既存鍵 import 抑止、
/// authentication subkey keygrip 登録、SSH agent socket と authentication subkey 公開鍵識別の順序を
/// application の順序制御として固定する。envelope 検証・recipient 照合・fingerprint 一致・既存鍵衝突・
/// import 済み鍵の防御的な subkey 利用可能性・gpg-agent SSH support 充足のいずれかで停止条件に達した場合は、
/// 後続の SSH 公開鍵経路へ進ませない。secret key material・DEK・復号済み backup はすべて port 境界の
/// 保護値として扱い、application 層では加工しない。順序を application に固定するのは、「import 前に
/// fingerprint を確定し既存鍵衝突を止める」「SSH support 確認まで進んだ鍵だけを復元完了とする」
/// という停止条件の責務境界を保護するためである。
pub(crate) async fn run_restore_gpg<B>(
    command: RestoreGpgCommand,
    runtime: RestoreGpgRuntime<'_, B>,
) -> Result<()>
where
    B: ports::bw::VaultClientPort,
{
    let RestoreGpgRuntime {
        device,
        process,
        secret_input,
        storage: storage_port,
        vault_client,
        recipient,
        cipher,
        keyring,
        ssh_agent,
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

    // 1-2. vault adapter が使う account API key を YubiKey storage から読み出し、master password を
    //      input port から取得してから、個人 vault から envelope を取得する。
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
    let secret_id = VaultSecretName::GpgSecretKeyBackup
        .resolve_id(vault_client.list_vault_secrets(&credentials).await?)?;

    // 3. envelope 形式（version / metadata / recipients / ciphertext）を検証して取得する。
    let envelope = vault_client
        .fetch_gpg_backup_envelope(&credentials, &secret_id)
        .await?;

    // 3-4. 2 recipient 以上の復旧到達状態を確認してから、接続中 YubiKey に一致する
    // public key fingerprint recipient を解決し、DEK を unwrap して backup を復号する。
    envelope.ensure_recovery_recipient_count()?;
    let connected = recipient.resolve_connected_recipient(serial)?;
    let matched = envelope.resolve_recipient(&connected)?;
    let dek = recipient.unwrap_dek(serial, matched, pin.as_ref())?;
    let backup = cipher.decrypt_backup(&dek, envelope.ciphertext())?;

    // 5. 復号済み backup から primary fingerprint を導出し、metadata と一致を検証する。
    let parsed_fingerprint = keyring.parse_backup_primary_fingerprint(&backup)?;
    if parsed_fingerprint.as_str() != envelope.metadata().primary_fingerprint().as_str() {
        anyhow::bail!(
            "decrypted gpg backup primary fingerprint does not match the envelope metadata"
        );
    }

    // 6. 同一 primary fingerprint の secret key が既に鍵リングにある場合は停止する。
    if keyring.secret_key_exists(&parsed_fingerprint)? {
        anyhow::bail!(
            "a GPG secret key with this primary fingerprint already exists; refusing to import"
        );
    }

    // 7. import する。import 後の手順 8-10 のいずれかで失敗した場合は、不完全な状態の secret key を
    // 鍵リングに残さないよう best-effort で削除してから元エラーを返す。残置すると次回 restore が手順 6 の
    // 既存鍵衝突で止まり復旧不能になるため、import 後の復元処理全体を atomic に扱う。
    let imported = keyring.import_secret_key(&backup)?;
    let restore_result = (|| -> Result<()> {
        // 8. import 後鍵の subkey 構成を実装ローカルの防御的な利用可能性条件として検証する。
        keyring
            .inspect_imported_key(&imported)
            .and_then(|composition| composition.ensure_usable())?;

        // 9. authentication subkey の keygrip を gpg-agent の SSH key list へ登録する（冪等）。
        let keygrip = keyring.authentication_subkey_keygrip(&imported)?;
        ssh_agent.register_authentication_subkey(&keygrip)?;

        // 10. gpg-agent SSH support 利用可否を確認する。identity 識別は authentication subkey 由来の
        // OpenSSH 公開鍵 key blob を期待値として照合するため、公開鍵を解決して渡す。
        let public_key = keyring.authentication_subkey_ssh_public_key(&imported)?;
        ssh_agent.inspect_ssh_agent(&public_key)?.ensure_ready()
    })();
    match restore_result {
        Ok(()) => report.write_restore_gpg_report(&RestoreGpgSummary {
            ssh_key_registered: true,
            ssh_support_ready: true,
        }),
        Err(error) => {
            let _ = keyring.delete_secret_key(&imported);
            Err(error)
        }
    }
}
