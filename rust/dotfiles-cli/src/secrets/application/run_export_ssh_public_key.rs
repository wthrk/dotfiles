//! export-ssh-public-key の順序を固定し、鍵リング解決と出力の実装詳細を port 境界の外へ閉じる。

use crate::Result;
use crate::secrets::{
    domain::{commands::ExportSshPublicKeyCommand, gpg_backup::SecretPrimaryKeyCandidates},
    ports,
};

/// ローカル鍵リング上の GPG authentication subkey 由来の OpenSSH 公開鍵を stdout へ出力する。
///
/// 設計「公開鍵出力契約」を満たす単純な順序制御だけを担う。公開鍵は秘密情報ではないため
/// `SshPublicKeyOutputPort` を通じて terminal でも出力を許可する。GitHub API 呼び出しや鍵サーバー
/// 参照は行わず、authentication subkey 由来の 1 行だけを機械可読な形式で渡す。順序を application に
/// 置くのは、鍵リング解決（adapter）と出力境界（adapter）の責務を分離したうえで「authentication
/// subkey の公開鍵を解決してから出力する」という手順だけを保持するためである。
pub(crate) fn run_export_ssh_public_key<K, O>(
    command: ExportSshPublicKeyCommand,
    keyring: &mut K,
    store: &dyn ports::PasswordStorePort,
    output: &O,
) -> Result<()>
where
    K: ports::GpgKeyringPort,
    O: ports::SshPublicKeyOutputPort,
{
    let _ = command;
    let primary_fingerprint = if store.password_store_exists()? {
        let readiness = store.inspect_password_store()?;
        if readiness.gpg_id_present && !readiness.gpg_id_recipients.is_empty() {
            let mut fingerprints = Vec::new();
            for recipient in readiness.parse_recipients()? {
                let Some(fingerprint) = keyring.primary_fingerprint_for_recipient(&recipient)?
                else {
                    anyhow::bail!(
                        "password-store recipient does not resolve to an available GPG secret key"
                    );
                };
                fingerprints.push(fingerprint);
            }
            SecretPrimaryKeyCandidates::new(fingerprints).resolve_unique()?
        } else {
            keyring
                .list_secret_primary_fingerprints()?
                .resolve_unique()?
        }
    } else {
        keyring
            .list_secret_primary_fingerprints()?
            .resolve_unique()?
    };
    let public_key = keyring.authentication_subkey_ssh_public_key(&primary_fingerprint)?;
    output.write_ssh_public_key(&public_key)
}

#[cfg(test)]
mod tests {
    //! export-ssh-public-key の順序（鍵リング解決→出力）を mockall で検証する単体テスト。
    //!
    //! 鍵リング backend と出力境界を port mock で差し替え、authentication subkey の OpenSSH 公開鍵が
    //! 解決され出力 port へ渡ることを確認する。test double は持ち込まない。

    use crate::secrets::{
        domain::{
            commands::ExportSshPublicKeyCommand,
            gpg_backup::{PrimaryFingerprint, SecretPrimaryKeyCandidates},
            gpg_restore::OpenSshPublicKey,
            pass_restore::PasswordStoreReadiness,
        },
        ports,
    };

    use super::run_export_ssh_public_key;

    const PRIMARY_FP: &str = "0123456789abcdef0123456789abcdef01234567";
    const SSH_LINE: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITESTBODY comment";

    #[test]
    fn export_ssh_public_key_resolves_and_writes() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_list_secret_primary_fingerprints()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| {
                Ok(SecretPrimaryKeyCandidates::new(vec![
                    PrimaryFingerprint::parse(PRIMARY_FP)?,
                ]))
            });
        keyring
            .expect_authentication_subkey_ssh_public_key()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| OpenSshPublicKey::parse(SSH_LINE));
        let mut output = ports::MockSshPublicKeyOutputPort::new();
        output
            .expect_write_ssh_public_key()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|public_key| public_key.as_str() == SSH_LINE)
            .returning(|_| Ok(()));

        let mut store = ports::MockPasswordStorePort::new();
        store
            .expect_password_store_exists()
            .times(1)
            .returning(|| Ok(false));

        let _ = PrimaryFingerprint::parse(PRIMARY_FP)?;

        run_export_ssh_public_key(ExportSshPublicKeyCommand, &mut keyring, &store, &output)
    }

    /// fingerprint 未指定時は鍵リング内の単一 secret primary を解決して SSH 公開鍵 export へ進む。
    #[test]
    fn export_ssh_public_key_without_fingerprint_resolves_single_secret_key() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring
            .expect_list_secret_primary_fingerprints()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| {
                Ok(SecretPrimaryKeyCandidates::new(vec![
                    PrimaryFingerprint::parse(PRIMARY_FP)?,
                ]))
            });
        keyring
            .expect_authentication_subkey_ssh_public_key()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|fingerprint| fingerprint.as_str() == PRIMARY_FP)
            .returning(|_| OpenSshPublicKey::parse(SSH_LINE));
        let mut output = ports::MockSshPublicKeyOutputPort::new();
        output
            .expect_write_ssh_public_key()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|public_key| public_key.as_str() == SSH_LINE)
            .returning(|_| Ok(()));

        let mut store = ports::MockPasswordStorePort::new();
        store
            .expect_password_store_exists()
            .times(1)
            .returning(|| Ok(false));

        run_export_ssh_public_key(ExportSshPublicKeyCommand, &mut keyring, &store, &output)
    }

    /// 設定済み password-store の `.gpg-id` recipient が解決できる場合は、その primary を優先する。
    #[test]
    fn export_ssh_public_key_prefers_configured_password_store_recipient() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut keyring = ports::MockGpgKeyringPort::new();
        keyring.expect_list_secret_primary_fingerprints().times(0);
        keyring
            .expect_primary_fingerprint_for_recipient()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|recipient| {
                assert_eq!(recipient.as_str(), PRIMARY_FP);
                Ok(Some(PrimaryFingerprint::parse(PRIMARY_FP)?))
            });
        keyring
            .expect_authentication_subkey_ssh_public_key()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|fingerprint| fingerprint.as_str() == PRIMARY_FP)
            .returning(|_| OpenSshPublicKey::parse(SSH_LINE));

        let mut store = ports::MockPasswordStorePort::new();
        store
            .expect_password_store_exists()
            .times(1)
            .returning(|| Ok(true));
        store
            .expect_inspect_password_store()
            .times(1)
            .returning(|| {
                Ok(PasswordStoreReadiness {
                    gpg_id_present: true,
                    gpg_id_recipients: vec![PRIMARY_FP.to_owned()],
                    sample_entry: None,
                })
            });

        let mut output = ports::MockSshPublicKeyOutputPort::new();
        output
            .expect_write_ssh_public_key()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(()));

        run_export_ssh_public_key(ExportSshPublicKeyCommand, &mut keyring, &store, &output)
    }
}
