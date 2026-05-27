#[cfg(feature = "secrets-test-stub")]
use std::env;

use anyhow::Result;
#[cfg(feature = "secrets-test-stub")]
use dotfiles_cli_secrets_test_contract::{
    ADAPTER_ROUTE_AUDIT_PREFIX, TEST_STUB_CONTEXT_ENV, TEST_STUB_CONTEXT_VALUE, USE_TEST_STUB_ENV,
};

#[cfg(feature = "secrets-test-stub")]
use crate::secrets::adapters::piv_io::device_test_stub::{
    TestStubDeviceAdapter, TestStubSecretDevice,
};
#[cfg(feature = "secrets-test-stub")]
use crate::secrets::ports::SecretDevice;
use crate::secrets::{
    adapters::yubikey::RealDeviceAdapter,
    ports::{DeviceCandidate, DeviceSelectionPort},
};

use super::DeviceAdapterRouteLabel;

#[cfg(not(feature = "secrets-test-stub"))]
const ADAPTER_ROUTE_AUDIT_PREFIX: &str = "DOTFILES_SECRETS_DEVICE_ADAPTER_ROUTE";

/// 同一 production command path 上で device 選択 route を確定する adapter。
///
/// 既定では実機 `real` route を使うが、`secrets-test-stub` feature 有効時のみ、
/// 許可済み env 条件ペアがそろった場合に限って `stub` route へ分岐する。
/// いずれの route も同一の command path / port 契約を通る same-route 検証境界内で扱う。
pub(crate) struct SelectedDeviceAdapter {
    inner: DeviceSelectionInner,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeviceAdapterRoute {
    Real,
    #[cfg(feature = "secrets-test-stub")]
    Stub,
}

impl DeviceAdapterRoute {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Real => "real",
            #[cfg(feature = "secrets-test-stub")]
            Self::Stub => "stub",
        }
    }
}

impl Default for SelectedDeviceAdapter {
    fn default() -> Self {
        Self::new()
    }
}

enum DeviceSelectionInner {
    Real(RealDeviceAdapter),
    #[cfg(feature = "secrets-test-stub")]
    Stub(TestStubDeviceAdapter),
}

#[cfg(feature = "secrets-test-stub")]
pub(crate) enum SelectedSecretDevice {
    Real(<RealDeviceAdapter as DeviceSelectionPort>::Device),
    Stub(TestStubSecretDevice),
}

#[cfg(feature = "secrets-test-stub")]
impl SecretDevice for SelectedSecretDevice {
    fn key_exists(&mut self) -> Result<bool> {
        match self {
            Self::Real(device) => device.key_exists(),
            Self::Stub(device) => device.key_exists(),
        }
    }

    fn check_key_generation_preconditions(&mut self) -> Result<()> {
        match self {
            Self::Real(device) => device.check_key_generation_preconditions(),
            Self::Stub(device) => device.check_key_generation_preconditions(),
        }
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        match self {
            Self::Real(device) => device.check_management_auth_preconditions(),
            Self::Stub(device) => device.check_management_auth_preconditions(),
        }
    }

    fn generate_key(&mut self) -> Result<()> {
        match self {
            Self::Real(device) => device.generate_key(),
            Self::Stub(device) => device.generate_key(),
        }
    }

    fn read_object(
        &mut self,
        object_id: crate::secrets::domain::piv::PivObjectId,
    ) -> Result<Option<Vec<u8>>> {
        match self {
            Self::Real(device) => device.read_object(object_id),
            Self::Stub(device) => device.read_object(object_id),
        }
    }

    fn write_object(
        &mut self,
        object_id: crate::secrets::domain::piv::PivObjectId,
        value: &mut [u8],
    ) -> Result<()> {
        match self {
            Self::Real(device) => device.write_object(object_id, value),
            Self::Stub(device) => device.write_object(object_id, value),
        }
    }

    fn wrap_key(
        &mut self,
        key: &crate::secrets::domain::material::SecretMaterial,
    ) -> Result<Vec<u8>> {
        match self {
            Self::Real(device) => device.wrap_key(key),
            Self::Stub(device) => device.wrap_key(key),
        }
    }

    fn requires_pin_input(&self) -> bool {
        match self {
            Self::Real(device) => device.requires_pin_input(),
            Self::Stub(device) => device.requires_pin_input(),
        }
    }

    fn verify_pin(&mut self, pin: &crate::secrets::domain::material::SecretMaterial) -> Result<()> {
        match self {
            Self::Real(device) => device.verify_pin(pin),
            Self::Stub(device) => device.verify_pin(pin),
        }
    }

    fn unwrap_key(
        &mut self,
        wrapped_key: &[u8],
    ) -> Result<crate::secrets::domain::material::SecretMaterial> {
        match self {
            Self::Real(device) => device.unwrap_key(wrapped_key),
            Self::Stub(device) => device.unwrap_key(wrapped_key),
        }
    }

    fn seal_for_storage(
        &mut self,
        storage: crate::secrets::domain::piv::SecretStorageSpec,
        plaintext: &crate::secrets::domain::material::SecretMaterial,
    ) -> Result<Vec<u8>> {
        match self {
            Self::Real(device) => device.seal_for_storage(storage, plaintext),
            Self::Stub(device) => device.seal_for_storage(storage, plaintext),
        }
    }

    fn open_from_storage(
        &mut self,
        storage: crate::secrets::domain::piv::SecretStorageSpec,
        encoded: &[u8],
    ) -> Result<crate::secrets::domain::material::SecretMaterial> {
        match self {
            Self::Real(device) => device.open_from_storage(storage, encoded),
            Self::Stub(device) => device.open_from_storage(storage, encoded),
        }
    }
}

impl SelectedDeviceAdapter {
    /// `real`/`stub` の route 選択と監査出力を 1 箇所で固定する。
    ///
    /// same-route 検証で確認するべき分岐条件と `ADAPTER_ROUTE_AUDIT_PREFIX` 出力を
    /// この境界関数に集約し、呼び出し側が独自に route 判定や監査出力を増やさないようにする。
    /// caller / 運用側の責務はこの adapter をそのまま利用し、route 制御を
    /// `secrets-test-stub` + 許可済み env 条件以外へ拡張しないことに限定される。
    fn new() -> Self {
        #[cfg(feature = "secrets-test-stub")]
        {
            let use_stub = env::var(USE_TEST_STUB_ENV).as_deref() == Ok("true")
                && env::var(TEST_STUB_CONTEXT_ENV).as_deref() == Ok(TEST_STUB_CONTEXT_VALUE);
            if use_stub {
                eprintln!("{ADAPTER_ROUTE_AUDIT_PREFIX}=stub");
                return Self {
                    inner: DeviceSelectionInner::Stub(TestStubDeviceAdapter::default()),
                };
            }
        }

        eprintln!("{ADAPTER_ROUTE_AUDIT_PREFIX}=real");
        Self {
            inner: DeviceSelectionInner::Real(RealDeviceAdapter),
        }
    }
}

impl DeviceAdapterRouteLabel for SelectedDeviceAdapter {
    fn adapter_route_label(&self) -> &'static str {
        match self.inner {
            DeviceSelectionInner::Real(_) => DeviceAdapterRoute::Real.as_str(),
            #[cfg(feature = "secrets-test-stub")]
            DeviceSelectionInner::Stub(_) => DeviceAdapterRoute::Stub.as_str(),
        }
    }
}

impl DeviceSelectionPort for SelectedDeviceAdapter {
    #[cfg(not(feature = "secrets-test-stub"))]
    type Device = <RealDeviceAdapter as DeviceSelectionPort>::Device;
    #[cfg(feature = "secrets-test-stub")]
    type Device = SelectedSecretDevice;

    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>> {
        match &mut self.inner {
            DeviceSelectionInner::Real(inner) => inner.discover_devices(),
            #[cfg(feature = "secrets-test-stub")]
            DeviceSelectionInner::Stub(inner) => inner.discover_devices(),
        }
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<Self::Device> {
        match &mut self.inner {
            #[cfg(not(feature = "secrets-test-stub"))]
            DeviceSelectionInner::Real(inner) => inner.open_device_by_serial(serial),
            #[cfg(feature = "secrets-test-stub")]
            DeviceSelectionInner::Real(inner) => inner
                .open_device_by_serial(serial)
                .map(SelectedSecretDevice::Real),
            #[cfg(feature = "secrets-test-stub")]
            DeviceSelectionInner::Stub(inner) => inner
                .open_device_by_serial(serial)
                .map(SelectedSecretDevice::Stub),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{LazyLock, Mutex};

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[test]
    fn selected_device_adapter_uses_real_route_by_default() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        #[cfg(feature = "secrets-test-stub")]
        {
            // SAFETY: tests serialize process-wide env mutation via `ENV_LOCK`.
            unsafe {
                std::env::remove_var(USE_TEST_STUB_ENV);
                std::env::remove_var(TEST_STUB_CONTEXT_ENV);
            }
        }
        let adapter = SelectedDeviceAdapter::default();
        assert_eq!(adapter.adapter_route_label(), "real");
    }

    #[cfg(feature = "secrets-test-stub")]
    #[test]
    fn selected_device_adapter_uses_stub_route_only_with_explicit_env_pair() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // SAFETY: tests serialize process-wide env mutation via `ENV_LOCK`.
        unsafe {
            std::env::set_var(USE_TEST_STUB_ENV, "true");
            std::env::set_var(TEST_STUB_CONTEXT_ENV, TEST_STUB_CONTEXT_VALUE);
        }
        let adapter = SelectedDeviceAdapter::default();
        assert_eq!(adapter.adapter_route_label(), "stub");
        // SAFETY: tests serialize process-wide env mutation via `ENV_LOCK`.
        unsafe {
            std::env::remove_var(USE_TEST_STUB_ENV);
            std::env::remove_var(TEST_STUB_CONTEXT_ENV);
        }
    }

    #[cfg(feature = "secrets-test-stub")]
    #[test]
    fn selected_device_adapter_rejects_partial_stub_env() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // SAFETY: tests serialize process-wide env mutation via `ENV_LOCK`.
        unsafe {
            std::env::set_var(USE_TEST_STUB_ENV, "true");
            std::env::set_var(TEST_STUB_CONTEXT_ENV, "unexpected");
        }
        let adapter = SelectedDeviceAdapter::default();
        assert_eq!(adapter.adapter_route_label(), "real");
        // SAFETY: tests serialize process-wide env mutation via `ENV_LOCK`.
        unsafe {
            std::env::remove_var(USE_TEST_STUB_ENV);
            std::env::remove_var(TEST_STUB_CONTEXT_ENV);
        }
    }
}
