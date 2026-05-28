//! secrets adapter 層の公開境界。
//!
//! adapter 下位 module をそのまま露出せず、entrypoint が使う runtime adapter 生成だけを提供する。

mod piv_io;

#[cfg(not(feature = "secrets-internal-test-stub"))]
use crate::secrets::support::protection::{ProtectedSecret, secret_consumer};
use std::collections::BTreeMap;
#[cfg(not(feature = "secrets-internal-test-stub"))]
use std::io::Write;
#[cfg(not(feature = "secrets-internal-test-stub"))]
use std::process::Command;
#[cfg(not(feature = "secrets-internal-test-stub"))]
use zeroize::Zeroizing;

use crate::{
    Result,
    secrets::{
        domain::{
            material::SecretMaterial,
            storage::{
                SecretStorageReadInspection, SecretStorageReadIntent, SecretStorageSetupInspection,
                SecretStorageSetupIntent, SecretStorageSetupProbe, SecretStorageWriteInspection,
                SecretStorageWriteIntent,
            },
            values::{BwsSecretName, EnrollSummary, VerifySummary},
        },
        ports::{
            BootstrapSecretDocumentInputPort, BwsClientPort, DevicePinPolicyPort, DeviceSerialPort,
            PinInputPort, ReportPort, RotationContinuationPort, SecretInputPort, SecretOutputPort,
            SecretStoragePort, SpareDeviceSerialPort,
        },
    },
};

/// CLI entrypoint が利用する secrets runtime adapter。
///
/// 公開面は port trait 実装型としてのこの型に限定し、下位 adapter module や
/// factory/helper 関数を crate 公開しない。
#[derive(Default)]
pub(crate) struct SecretsAdapters {
    device: piv_io::DeviceSelectionAdapter,
    process_io: piv_io::ProcessIoAdapter,
    storage: piv_io::StorageAdapter,
    report: piv_io::JsonReportAdapter,
    bws_client: BwsClientAdapter,
}

impl DeviceSerialPort for SecretsAdapters {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
        self.device.resolve_device_serial(requested)
    }
}

impl SpareDeviceSerialPort for SecretsAdapters {
    fn resolve_spare_device_serial(&mut self, requested_spare_serial: Option<u32>) -> Result<u32> {
        self.device
            .resolve_spare_device_serial(requested_spare_serial)
    }
}

impl DevicePinPolicyPort for SecretsAdapters {
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool> {
        self.device.device_requires_pin(serial)
    }
}

impl PinInputPort for SecretsAdapters {
    fn read_pin(&self) -> Result<SecretMaterial> {
        self.process_io.read_pin()
    }
}

impl SecretInputPort for SecretsAdapters {
    fn read_bw_email_secret(&self) -> Result<SecretMaterial> {
        self.process_io.read_bw_email_secret()
    }

    fn read_bw_password_secret(&self) -> Result<SecretMaterial> {
        self.process_io.read_bw_password_secret()
    }

    fn read_bws_access_token_secret(&self) -> Result<SecretMaterial> {
        self.process_io.read_bws_access_token_secret()
    }

    fn read_streamed_secret(&self) -> Result<SecretMaterial> {
        self.process_io.read_streamed_secret()
    }
}

impl RotationContinuationPort for SecretsAdapters {
    fn continue_rotation(&self) -> Result<bool> {
        self.process_io.continue_rotation()
    }
}

impl BootstrapSecretDocumentInputPort for SecretsAdapters {
    fn read_bootstrap_secret_fields(&self) -> Result<BTreeMap<String, SecretMaterial>> {
        self.process_io.read_bootstrap_secret_fields()
    }
}

impl SecretOutputPort for SecretsAdapters {
    fn write_secret(&self, secret: &SecretMaterial) -> Result<()> {
        self.process_io.write_secret(secret)
    }
}

impl SecretStoragePort for SecretsAdapters {
    fn inspect_secret_storage_setup(
        &mut self,
        serial: u32,
        probe: &SecretStorageSetupProbe,
    ) -> Result<SecretStorageSetupInspection> {
        self.storage.inspect_secret_storage_setup(serial, probe)
    }

    fn initialize_secret_storage(
        &mut self,
        serial: u32,
        intent: SecretStorageSetupIntent,
    ) -> Result<()> {
        self.storage.initialize_secret_storage(serial, intent)
    }

    fn finalize_secret_storage_setup(
        &mut self,
        serial: u32,
        intent: SecretStorageSetupIntent,
    ) -> Result<()> {
        self.storage.finalize_secret_storage_setup(serial, intent)
    }

    fn inspect_secret_storage_write(
        &mut self,
        serial: u32,
        storage: &crate::secrets::domain::piv::SecretStorageSpec,
    ) -> Result<SecretStorageWriteInspection> {
        self.storage.inspect_secret_storage_write(serial, storage)
    }

    fn store_secret(
        &mut self,
        serial: u32,
        intent: SecretStorageWriteIntent,
        secret: &SecretMaterial,
    ) -> Result<()> {
        self.storage.store_secret(serial, intent, secret)
    }

    fn inspect_secret_storage_read(
        &mut self,
        serial: u32,
        storage: &crate::secrets::domain::piv::SecretStorageSpec,
    ) -> Result<SecretStorageReadInspection> {
        self.storage.inspect_secret_storage_read(serial, storage)
    }

    fn load_secret(
        &mut self,
        serial: u32,
        intent: &SecretStorageReadIntent,
        pin: Option<&SecretMaterial>,
    ) -> Result<SecretMaterial> {
        self.storage.load_secret(serial, intent, pin)
    }
}

impl ReportPort for SecretsAdapters {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()> {
        self.report.write_enroll_report(summary)
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        self.report.write_verify_report(summary)
    }
}

impl BwsClientPort for SecretsAdapters {
    fn fetch_bws_secret(
        &self,
        access_token: &SecretMaterial,
        secret_name: BwsSecretName,
    ) -> Result<SecretMaterial> {
        self.bws_client.fetch_bws_secret(access_token, secret_name)
    }
}

/// verify-yubikey の external check で使う BWS 境界 adapter。
///
/// access token は `SecretMaterial` の protection backend から必要最小限だけ展開し、
/// CLI 引き渡し後は `Zeroizing` で破棄時消去する。adapter は `bws` 実行と
/// JSON 変換のみを担当し、use case 手順や判定は application/domain に残す。
#[derive(Default)]
struct BwsClientAdapter;

#[cfg(not(feature = "secrets-internal-test-stub"))]
struct ZeroizingVecWriter<'a>(&'a mut Zeroizing<Vec<u8>>);

#[cfg(not(feature = "secrets-internal-test-stub"))]
impl Write for ZeroizingVecWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl BwsClientPort for BwsClientAdapter {
    fn fetch_bws_secret(
        &self,
        access_token: &SecretMaterial,
        secret_name: BwsSecretName,
    ) -> Result<SecretMaterial> {
        #[cfg(feature = "secrets-internal-test-stub")]
        {
            let _ = access_token;
            let value = match secret_name {
                BwsSecretName::GpgSecretKeyBackup => {
                    b"-----BEGIN PGP PRIVATE KEY BLOCK-----\nmock\n-----END PGP PRIVATE KEY BLOCK-----\n"
                        .to_vec()
                }
                BwsSecretName::PasswordStoreRemote => {
                    b"git@github.com:example/password-store.git".to_vec()
                }
            };
            Ok(SecretMaterial::from_backend(
                value,
                |secret| secret.len(),
                |secret| Ok(secret.clone()),
            ))
        }

        #[cfg(not(feature = "secrets-internal-test-stub"))]
        {
            let protected = access_token
                .as_backend::<ProtectedSecret>()
                .ok_or_else(|| {
                    anyhow::anyhow!("bws access token backend is not protected memory")
                })?;
            let mut token_bytes = Zeroizing::new(Vec::<u8>::new());
            let mut writer = ZeroizingVecWriter(&mut token_bytes);
            secret_consumer::write_to(protected, &mut writer)?;
            let token = Zeroizing::new(
                String::from_utf8(std::mem::take(&mut *token_bytes))
                    .map_err(|_| anyhow::anyhow!("bws access token is not valid UTF-8"))?,
            );
            let key = match secret_name {
                BwsSecretName::GpgSecretKeyBackup => "gpg-secret-key-backup",
                BwsSecretName::PasswordStoreRemote => "password-store-remote",
            };
            let output = Command::new("bws")
                .args([
                    "secret",
                    "get",
                    key,
                    "--access-token",
                    token.trim(),
                    "--output",
                    "json",
                ])
                .output()
                .map_err(|error| anyhow::anyhow!("failed to invoke bws CLI: {error}"))?;
            if !output.status.success() {
                let status = output.status.code().map_or_else(
                    || "terminated by signal".to_string(),
                    |code| code.to_string(),
                );
                return Err(anyhow::anyhow!(
                    "bws external check failed for {key} (exit status: {status})"
                ));
            }
            let payload: serde_json::Value = serde_json::from_slice(&output.stdout)
                .map_err(|error| anyhow::anyhow!("failed to decode bws secret JSON: {error}"))?;
            let value = payload
                .get("value")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("bws secret response does not contain value"))?;
            Ok(SecretMaterial::from_backend(
                value.as_bytes().to_vec(),
                |secret| secret.len(),
                |secret| Ok(secret.clone()),
            ))
        }
    }
}
