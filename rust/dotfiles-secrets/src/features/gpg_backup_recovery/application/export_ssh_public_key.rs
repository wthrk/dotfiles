//! export-ssh-public-key の順序を固定し、鍵リング解決と出力の実装詳細を port 境界の外へ閉じる。

use crate::Result;
use crate::{
    features::gpg_backup_recovery::domain::commands::ExportSshPublicKeyCommand,
    features::{
        cli_interaction::ports::public::SshPublicKeyOutputPort,
        gpg_backup_recovery::ports::public::GpgKeyringPort,
    },
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
    output: &O,
) -> Result<()>
where
    K: GpgKeyringPort + ?Sized,
    O: SshPublicKeyOutputPort + ?Sized,
{
    let public_key = keyring.authentication_subkey_ssh_public_key(&command.primary_fingerprint)?;
    output.write_ssh_public_key(&public_key)
}

#[cfg(test)]
mod tests {
    //! export-ssh-public-key の順序（鍵リング解決→出力）を mockall で検証する単体テスト。
    //!
    //! 鍵リング backend と出力境界を port mock で差し替え、authentication subkey の OpenSSH 公開鍵が
    //! 解決され出力 port へ渡ることを確認する。test double は持ち込まない。

    use crate::features::{
        cli_interaction::ports::io::MockSshPublicKeyOutputPort,
        gpg_backup_recovery::{
            domain::{
                commands::ExportSshPublicKeyCommand, gpg_backup::PrimaryFingerprint,
                gpg_restore::OpenSshPublicKey,
            },
            ports::gpg::MockGpgKeyringPort,
        },
    };

    use super::run_export_ssh_public_key;

    const PRIMARY_FP: &str = "0123456789abcdef0123456789abcdef01234567";
    const SSH_LINE: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITESTBODY comment";

    #[test]
    fn export_ssh_public_key_resolves_and_writes() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let mut keyring = MockGpgKeyringPort::new();
        keyring
            .expect_authentication_subkey_ssh_public_key()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| OpenSshPublicKey::parse(SSH_LINE));
        let mut output = MockSshPublicKeyOutputPort::new();
        output
            .expect_write_ssh_public_key()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|public_key| public_key.as_str() == SSH_LINE)
            .returning(|_| Ok(()));

        run_export_ssh_public_key(
            ExportSshPublicKeyCommand {
                primary_fingerprint: PrimaryFingerprint::parse(PRIMARY_FP)?,
            },
            &mut keyring,
            &output,
        )
    }
}
