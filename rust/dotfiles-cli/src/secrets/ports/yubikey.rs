//! YubiKey backend capability の port 契約。

use crate::Result;
use crate::secrets::domain::{
    piv::SecretStorageSpec,
    storage::{
        SecretStorageReadInspection, SecretStorageReadIntent, SecretStorageSetupInspection,
        SecretStorageSetupIntent, SecretStorageSetupProbe, SecretStorageWriteInspection,
        SecretStorageWriteIntent,
    },
};
use crate::secrets::support::protection::ProtectedSecret;

#[cfg_attr(test, mockall::automock)]
pub trait DeviceSerialPort {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32>;
}

#[cfg_attr(test, mockall::automock)]
pub trait DevicePinPolicyPort {
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool>;
}

#[cfg_attr(test, mockall::automock)]
pub trait SpareDeviceSerialPort {
    fn resolve_spare_device_serial(&mut self, requested_spare_serial: Option<u32>) -> Result<u32>;
}

#[cfg_attr(test, mockall::automock)]
pub trait SecretStoragePort {
    fn inspect_secret_storage_setup(
        &mut self,
        serial: u32,
        probe: &SecretStorageSetupProbe,
    ) -> Result<SecretStorageSetupInspection>;

    fn initialize_secret_storage(
        &mut self,
        serial: u32,
        intent: SecretStorageSetupIntent,
    ) -> Result<()>;

    fn finalize_secret_storage_setup(
        &mut self,
        serial: u32,
        intent: SecretStorageSetupIntent,
    ) -> Result<()>;

    fn inspect_secret_storage_write(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageWriteInspection>;

    fn store_secret(
        &mut self,
        serial: u32,
        intent: SecretStorageWriteIntent,
        secret: &ProtectedSecret,
    ) -> Result<()>;

    fn inspect_secret_storage_read(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageReadInspection>;

    #[expect(
        clippy::needless_lifetimes,
        reason = "mockall::automock 展開のため named lifetime が必要"
    )]
    fn load_secret<'a>(
        &mut self,
        serial: u32,
        intent: &SecretStorageReadIntent,
        pin: Option<&'a ProtectedSecret>,
    ) -> Result<ProtectedSecret>;
}
