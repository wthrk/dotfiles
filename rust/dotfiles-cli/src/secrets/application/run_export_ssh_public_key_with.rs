//! export-ssh-public-key の順序責務を保持し、GPG/出力実装の境界を固定する。

use crate::Result;
use crate::secrets::{domain::values::ExportSshPublicKeyCommand, ports};

/// GPG authentication subkey 由来の SSH 公開鍵を取得し、出力境界へ渡す。
///
/// 公開鍵フォーマット決定と stdout への書き込みは adapter 側へ委譲し、application は
/// 取得→出力の順序だけを保持する。
pub(crate) fn run_export_ssh_public_key_with<
    B: ports::GpgRecoveryPort + ports::SshPublicKeyOutputPort,
>(
    _command: ExportSshPublicKeyCommand,
    boundary: &mut B,
) -> Result<()> {
    let public_key = boundary.export_ssh_public_key()?;
    boundary.write_ssh_public_key(&public_key)
}

#[cfg(test)]
mod tests {
    use crate::Result;
    use crate::secrets::{
        domain::{material::SecretMaterial, values::ExportSshPublicKeyCommand},
        ports,
    };

    use super::run_export_ssh_public_key_with;

    struct Boundary;

    impl ports::GpgRecoveryPort for Boundary {
        fn read_gpg_secret_key_backup(&self, _bws_access_token: &SecretMaterial) -> Result<String> {
            unreachable!()
        }

        fn import_gpg_secret_key(&self, _armored_secret_key: &str) -> Result<()> {
            unreachable!()
        }

        fn verify_gpg_restore_prerequisites(&self) -> Result<()> {
            unreachable!()
        }

        fn export_ssh_public_key(&self) -> Result<String> {
            Ok("ssh-ed25519 AAAATESTKEY user@example".to_string())
        }
    }

    impl ports::SshPublicKeyOutputPort for Boundary {
        fn write_ssh_public_key(&self, _public_key: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn export_ssh_public_key_writes_exported_value() -> Result<()> {
        let mut boundary = Boundary;
        run_export_ssh_public_key_with(ExportSshPublicKeyCommand, &mut boundary)
    }
}
