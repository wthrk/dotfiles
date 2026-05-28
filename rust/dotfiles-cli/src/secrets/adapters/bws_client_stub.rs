//! internal test stub feature 用の BWS adapter。

use crate::{
    Result,
    secrets::{
        domain::{material::SecretMaterial, values::BwsSecretName},
        ports::BwsClientPort,
        support::protection::{ProtectedSecret, secret_consumer},
    },
};

#[derive(Default)]
pub(crate) struct BwsClientAdapter;

impl BwsClientPort for BwsClientAdapter {
    fn fetch_bws_secret(
        &self,
        access_token: &SecretMaterial,
        secret_name: BwsSecretName,
    ) -> Result<SecretMaterial> {
        let protected = access_token
            .as_backend::<ProtectedSecret>()
            .ok_or_else(|| anyhow::anyhow!("bws access token backend is not protected memory"))?;
        let _ = secret_consumer::with_utf8_secret(protected, |_token| Ok(()))?;

        let value = match secret_name {
            BwsSecretName::GpgSecretKeyBackup => {
                b"-----BEGIN PGP PRIVATE KEY BLOCK-----\nmock\n-----END PGP PRIVATE KEY BLOCK-----\n"
                    .to_vec()
            }
            BwsSecretName::PasswordStoreRemote => b"git@github.com:example/password-store.git".to_vec(),
        };
        Ok(SecretMaterial::from_backend(
            value,
            |secret| secret.len(),
            |secret| Ok(secret.clone()),
        ))
    }
}
