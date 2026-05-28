//! YubiKey PIV discovery/selection と実プロセス I/O を port 契約へ接続する adapter。

#[path = "piv_io/device_selection.rs"]
mod device_selection;
#[path = "piv_io/process_io_adapter.rs"]
mod process_io_adapter;
#[path = "piv_io/report_adapter.rs"]
mod report_adapter;
#[path = "piv_io/storage_adapter.rs"]
mod storage_adapter;

#[cfg(feature = "secrets-internal-test-stub")]
#[path = "piv_io/selected_device_stub.rs"]
mod selected_device;
#[cfg(not(feature = "secrets-internal-test-stub"))]
#[path = "piv_io/selected_device_real.rs"]
mod selected_device;

pub(crate) use device_selection::DeviceSelectionAdapter;
pub(crate) use process_io_adapter::ProcessIoAdapter;
pub(crate) use report_adapter::JsonReportAdapter;
pub(crate) use storage_adapter::StorageAdapter;

use crate::{
    Result,
    secrets::{
        domain::{
            material::SecretMaterial,
            piv::{PivApplicationVersion, PivObjectId, SecretStorageSpec},
        },
        support::protection::{ProtectedSecret, secret_consumer},
    },
};

const ADAPTER_ROUTE_AUDIT_PREFIX: &str = "DOTFILES_SECRETS_DEVICE_ADAPTER_ROUTE";
#[cfg(feature = "secrets-internal-test-stub")]
const SELECTED_DEVICE_ROUTE_LABEL: &str = "stub";
#[cfg(not(feature = "secrets-internal-test-stub"))]
const SELECTED_DEVICE_ROUTE_LABEL: &str = "real";

fn selected_device_route_label() -> &'static str {
    SELECTED_DEVICE_ROUTE_LABEL
}

fn material_from_protected(protected: ProtectedSecret) -> SecretMaterial {
    SecretMaterial::from_backend(protected, ProtectedSecret::len, ProtectedSecret::try_clone)
}

fn protected_from_material(secret: &SecretMaterial) -> Result<&ProtectedSecret> {
    secret
        .as_backend::<ProtectedSecret>()
        .ok_or_else(|| anyhow::anyhow!("secret material backend is not protected memory"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeviceCandidate {
    serial: u32,
    label: String,
}

trait SecretDeviceIo {
    fn key_exists(&mut self) -> Result<bool>;
    fn piv_application_version(&self) -> PivApplicationVersion;
    fn pin_retries(&mut self) -> Result<u8>;
    fn check_management_auth_preconditions(&mut self) -> Result<()>;
    fn generate_key(&mut self) -> Result<()>;
    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>>;
    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()>;
    fn requires_pin_input(&self) -> bool;
    fn verify_pin(&mut self, pin: &SecretMaterial) -> Result<()>;
    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        plaintext: &SecretMaterial,
    ) -> Result<Vec<u8>>;
    fn open_from_storage(
        &mut self,
        storage: SecretStorageSpec,
        encoded: &[u8],
    ) -> Result<SecretMaterial>;
}

trait SelectedDeviceDiscoveryIo {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>>;
    fn open_device_by_serial(&mut self, serial: u32) -> Result<SelectedSecretDevice>;
}

struct SelectedDeviceAdapter;

impl Default for SelectedDeviceAdapter {
    fn default() -> Self {
        eprintln!(
            "{ADAPTER_ROUTE_AUDIT_PREFIX}={}",
            selected_device_route_label()
        );
        Self
    }
}

struct SelectedSecretDevice {
    inner: Box<dyn SecretDeviceIo>,
}

impl SelectedSecretDevice {
    fn new(device: impl SecretDeviceIo + 'static) -> Self {
        Self {
            inner: Box::new(device),
        }
    }
}

impl SecretDeviceIo for SelectedSecretDevice {
    fn key_exists(&mut self) -> Result<bool> {
        self.inner.key_exists()
    }

    fn piv_application_version(&self) -> PivApplicationVersion {
        self.inner.piv_application_version()
    }

    fn pin_retries(&mut self) -> Result<u8> {
        self.inner.pin_retries()
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        self.inner.check_management_auth_preconditions()
    }

    fn generate_key(&mut self) -> Result<()> {
        self.inner.generate_key()
    }

    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>> {
        self.inner.read_object(object_id)
    }

    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()> {
        self.inner.write_object(object_id, value)
    }

    fn requires_pin_input(&self) -> bool {
        self.inner.requires_pin_input()
    }

    fn verify_pin(&mut self, pin: &SecretMaterial) -> Result<()> {
        self.inner.verify_pin(pin)
    }

    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        plaintext: &SecretMaterial,
    ) -> Result<Vec<u8>> {
        self.inner.seal_for_storage(storage, plaintext)
    }

    fn open_from_storage(
        &mut self,
        storage: SecretStorageSpec,
        encoded: &[u8],
    ) -> Result<SecretMaterial> {
        self.inner.open_from_storage(storage, encoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_device_adapter_route_is_compile_time_selected() {
        assert!(matches!(selected_device_route_label(), "real" | "stub"));
    }
}
