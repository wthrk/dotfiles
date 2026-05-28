// application usecase test の port double を集約する共通 support。
//
// この file は `#[cfg(test)]` の test-only bridge から module context へ読み込まれる。
// mock/fake 本体は `tests/` 配下に置き、production build と production command path には含めない。

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::Result;
use crate::secrets::{
    domain::{
        manifest::SecretManifest,
        material::SecretMaterial,
        piv::{PivApplicationVersion, SecretName, SecretStorageSpec},
        storage::{
            SecretStorageReadInspection, SecretStorageReadIntent, SecretStorageSetupInspection,
            SecretStorageSetupIntent, SecretStorageSetupProbe, SecretStorageWriteInspection,
            SecretStorageWriteIntent,
        },
        values::{BwsSecretName, CheckName, CheckStatus, EnrollSummary, VerifySummary},
    },
    ports::{self, SecretStoragePort},
};

mockall::mock! {
    AppEventExpectation {
        fn hit_event(&mut self, event: &'static str);
    }
}

/// usecase port 呼び出しを直接受ける test support。
///
/// port method は HTTP route へ変換せず、共有 state を直接更新・参照する。呼び出し回数の
/// 期待値だけを mockall に委ね、application test から production adapter の経路を偽装しない。
pub(crate) struct AppMock {
    state: Mutex<AppMockState>,
    event_expectation: Mutex<MockAppEventExpectation>,
}

impl AppMock {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(AppMockState::default()),
            event_expectation: Mutex::new(MockAppEventExpectation::new()),
        }
    }

    pub(crate) fn expect_event(&mut self, event: &'static str) {
        self.expect_event_times(event, 1);
    }

    pub(crate) fn expect_event_times(&mut self, event: &'static str, hits: usize) {
        self.configure(|state| state.expected_events.insert(event, hits));
        self.event_expectation
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .expect_hit_event()
            .with(mockall::predicate::eq(event))
            .times(hits)
            .return_const(());
    }

    pub(crate) fn set_primary_serial(&self, serial: u32) {
        self.configure(|state| state.primary_serial = serial);
    }

    pub(crate) fn set_spare_serial(&self, serial: u32) {
        self.configure(|state| state.spare_serial = serial);
    }

    pub(crate) fn set_device_resolution_sequence(&self, serials: Vec<u32>) {
        self.configure(|state| state.device_resolution_sequence = serials);
    }

    pub(crate) fn set_rotation_continuations(&self, continuations: Vec<bool>) {
        self.configure(|state| state.rotation_continuations = continuations);
    }

    pub(crate) fn set_primary_available(&self, available: bool) {
        self.configure(|state| state.primary_available = available);
    }

    pub(crate) fn set_primary_requires_pin(&self, requires_pin: bool) {
        self.configure(|state| state.primary_requires_pin = requires_pin);
    }

    pub(crate) fn set_spare_requires_pin(&self, requires_pin: bool) {
        self.configure(|state| state.spare_requires_pin = requires_pin);
    }

    pub(crate) fn set_loaded_len(&self, len: usize) {
        self.configure(|state| state.loaded_len = len);
    }

    pub(crate) fn set_loaded_secret_value(&self, secret: SecretName, value: &'static [u8]) {
        self.configure(|state| {
            state.loaded_values.insert(secret, value.to_vec());
        });
    }

    pub(crate) fn stored_secret_value(&self, secret: SecretName) -> Option<Vec<u8>> {
        self.snapshot(|state| state.loaded_values.get(&secret).cloned())
    }

    pub(crate) fn output_secret_value(&self) -> Option<Vec<u8>> {
        self.snapshot(|state| state.output_secret.clone())
    }

    pub(crate) fn set_setup_failure(&self, fail: bool) {
        self.configure(|state| state.fail_setup = fail);
    }

    pub(crate) fn set_store_failure(&self, secret: SecretName) {
        self.configure(|state| state.fail_on_store = Some(secret));
    }

    pub(crate) fn set_store_already_updated_failure(&self, secret: SecretName) {
        self.configure(|state| {
            state.fail_on_store = Some(secret);
            state.store_failure_status = 409;
        });
    }

    pub(crate) fn set_pin_error(&self, error: &'static str) {
        self.configure(|state| state.pin_error = Some(error));
    }

    pub(crate) fn set_stdin_json_error(&self, error: &'static str) {
        self.configure(|state| state.stdin_json_error = Some(error));
    }

    pub(crate) fn set_streamed_secret_error(&self, error: &'static str) {
        self.configure(|state| state.streamed_secret_error = Some(error));
    }

    pub(crate) fn set_write_object_exists(&self, object_exists: bool) {
        self.configure(|state| state.write_object_exists = object_exists);
    }

    pub(crate) fn set_write_manifest_missing(&self) {
        self.configure(|state| state.write_manifest_exists = false);
    }

    pub(crate) fn set_secret_value(&self, secret: SecretName, value: &'static [u8]) {
        self.configure(|state| {
            state.secret_values.insert(secret, value.to_vec());
        });
    }

    pub(crate) fn set_secret_error(&self, secret: SecretName, error: &'static str) {
        self.configure(|state| {
            state.secret_errors.insert(secret, error);
        });
    }

    pub(crate) fn stores(&self) -> Vec<SecretName> {
        self.snapshot(|state| state.stores.clone())
    }

    pub(crate) fn resolution_order(&self) -> Vec<&'static str> {
        self.snapshot(|state| state.resolution_order.clone())
    }

    pub(crate) fn event_order(&self) -> Vec<&'static str> {
        self.snapshot(|state| state.event_order.clone())
    }

    pub(crate) fn reports(&self) -> Vec<VerifySummary> {
        self.snapshot(|state| {
            state
                .reports
                .iter()
                .map(|(serial, status)| verify_summary(*serial, *status))
                .collect()
        })
    }

    fn hit_event(&self, event: &'static str) {
        let should_verify = self.configure(|state| {
            state.hit_event(event);
            state.expected_events.contains_key(event)
        });
        if should_verify {
            self.event_expectation
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .hit_event(event);
        }
    }

    fn configure<T>(&self, update: impl FnOnce(&mut AppMockState) -> T) -> T {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        update(&mut state)
    }

    fn snapshot<T>(&self, read: impl FnOnce(&AppMockState) -> T) -> T {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        read(&state)
    }
}

impl Drop for AppMock {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let Ok(state) = self.state.lock() else {
            return;
        };
        for (event, expected) in &state.expected_events {
            let actual = state.event_hits.get(event).copied().unwrap_or_default();
            assert_eq!(
                actual, *expected,
                "mockall app event `{event}` hit count mismatch"
            );
        }
    }
}

/// application usecase が要求する port trait を直接実装する境界。
pub(crate) struct AppMockBoundary {
    pub(crate) mock: AppMock,
}

impl AppMockBoundary {
    pub(crate) fn new() -> Self {
        Self {
            mock: AppMock::new(),
        }
    }

    pub(crate) fn expect_setup(mut self) -> Self {
        self.mock.expect_event("setup");
        self
    }

    pub(crate) fn expect_setup_initialize(mut self) -> Self {
        self.mock.expect_event("setup-initialize");
        self
    }

    pub(crate) fn expect_setup_finalize(mut self) -> Self {
        self.mock.expect_event("setup-finalize");
        self
    }

    pub(crate) fn expect_store_times(mut self, hits: usize) -> Self {
        self.mock.expect_event_times("store", hits);
        self
    }

    pub(crate) fn expect_report(mut self) -> Self {
        self.mock.expect_event("report");
        self
    }

    pub(crate) fn expect_report_times(mut self, hits: usize) -> Self {
        self.mock.expect_event_times("report", hits);
        self
    }

    pub(crate) fn expect_pin(mut self) -> Self {
        self.mock.expect_event("pin");
        self
    }

    pub(crate) fn expect_enrollment_success(self) -> Self {
        self.expect_setup()
            .expect_setup_finalize()
            .expect_store_times(3)
            .expect_report()
    }

    pub(crate) fn expect_rotation_success(self) -> Self {
        self.expect_store_times(1).expect_report()
    }
}

impl ports::DeviceSerialPort for AppMockBoundary {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
        self.mock.configure(|state| {
            state.resolution_order.push("primary");
            if let Some(serial) = requested {
                return Ok(serial);
            }
            if !state.device_resolution_sequence.is_empty() {
                return Ok(state.device_resolution_sequence.remove(0));
            }
            requested
                .or(if state.primary_available {
                    Some(state.primary_serial)
                } else {
                    None
                })
                .ok_or_else(|| invalid_input("pass --serial in non-interactive use").into())
        })
    }
}

impl ports::SpareDeviceSerialPort for AppMockBoundary {
    fn resolve_spare_device_serial(
        &mut self,
        requested_spare_serial: Option<u32>,
    ) -> Result<u32> {
        self.mock.configure(|state| {
            state.resolution_order.push("spare");
            Ok(requested_spare_serial.unwrap_or(state.spare_serial))
        })
    }
}

impl ports::DevicePinPolicyPort for AppMockBoundary {
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool> {
        Ok(self.mock.snapshot(|state| {
            if serial == state.primary_serial {
                state.primary_requires_pin
            } else if serial == state.spare_serial {
                state.spare_requires_pin
            } else {
                false
            }
        }))
    }
}

impl ports::PinInputPort for AppMockBoundary {
    fn read_pin(&self) -> Result<SecretMaterial> {
        self.mock.hit_event("pin");
        self.mock.snapshot(|state| {
            state
                .pin_error
                .map(|message| Err(anyhow::anyhow!(message)))
                .unwrap_or_else(|| Ok(secret_material(b"123456".to_vec())))
        })
    }
}

impl ports::SecretInputPort for AppMockBoundary {
    fn read_bw_email_secret(&self) -> Result<SecretMaterial> {
        self.read_secret_value(SecretName::BwEmail)
    }

    fn read_bw_password_secret(&self) -> Result<SecretMaterial> {
        self.read_secret_value(SecretName::BwPassword)
    }

    fn read_bws_access_token_secret(&self) -> Result<SecretMaterial> {
        self.read_secret_value(SecretName::BwsAccessToken)
    }

    fn read_streamed_secret(&self) -> Result<SecretMaterial> {
        self.mock.snapshot(|state| {
            state
                .streamed_secret_error
                .map(|message| Err(anyhow::anyhow!(message)))
                .unwrap_or_else(|| Ok(secret_material(b"token".to_vec())))
        })
    }
}

impl AppMockBoundary {
    fn read_secret_value(&self, secret: SecretName) -> Result<SecretMaterial> {
        self.mock.snapshot(|state| {
            state
                .secret_errors
                .get(&secret)
                .map(|message| Err(anyhow::anyhow!(*message)))
                .unwrap_or_else(|| Ok(secret_material(state.secret_value(secret))))
        })
    }
}

impl ports::RotationContinuationPort for AppMockBoundary {
    fn continue_rotation(&self) -> Result<bool> {
        self.mock.configure(|state| {
            state.hit_event("continue-rotation");
            Ok(if state.rotation_continuations.is_empty() {
                false
            } else {
                state.rotation_continuations.remove(0)
            })
        })
    }
}

impl ports::BootstrapSecretDocumentInputPort for AppMockBoundary {
    fn read_bootstrap_secret_fields(&self) -> Result<BTreeMap<String, SecretMaterial>> {
        self.mock.snapshot(|state| {
            if let Some(message) = state.stdin_json_error {
                return Err(anyhow::anyhow!(message));
            }
            Ok(BTreeMap::from([
                (
                    "bw-email".to_string(),
                    secret_material(state.secret_value(SecretName::BwEmail)),
                ),
                (
                    "bw-password".to_string(),
                    secret_material(state.secret_value(SecretName::BwPassword)),
                ),
                (
                    "bws-access-token".to_string(),
                    secret_material(state.secret_value(SecretName::BwsAccessToken)),
                ),
            ]))
        })
    }
}

impl ports::SecretOutputPort for AppMockBoundary {
    fn write_secret(&self, secret: &SecretMaterial) -> Result<()> {
        let body = secret_bytes(secret)?;
        self.mock.configure(|state| {
            state.output_secret = Some(body);
        });
        Ok(())
    }
}

impl ports::ReportPort for AppMockBoundary {
    fn write_enroll_report(&self, _summary: &EnrollSummary) -> Result<()> {
        self.mock.hit_event("report");
        Ok(())
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        self.mock.hit_event("report");
        let local_storage = summary
            .checks
            .get(&CheckName::LocalStorage)
            .copied()
            .unwrap_or(CheckStatus::Skipped);
        self.mock.configure(|state| {
            state.reports.push((summary.serial, local_storage));
        });
        Ok(())
    }
}

impl ports::BwsClientPort for AppMockBoundary {
    fn fetch_bws_secret(
        &self,
        _access_token: &SecretMaterial,
        secret_name: BwsSecretName,
    ) -> Result<SecretMaterial> {
        let value = match secret_name {
            BwsSecretName::GpgSecretKeyBackup => {
                b"-----BEGIN PGP PRIVATE KEY BLOCK-----\nmock\n-----END PGP PRIVATE KEY BLOCK-----\n"
                    .to_vec()
            }
            BwsSecretName::PasswordStoreRemote => b"git@github.com:example/password-store.git".to_vec(),
        };
        Ok(secret_material(value))
    }
}

impl SecretStoragePort for AppMockBoundary {
    fn inspect_secret_storage_setup(
        &mut self,
        _serial: u32,
        _probe: &SecretStorageSetupProbe,
    ) -> Result<SecretStorageSetupInspection> {
        self.mock.hit_event("setup");
        if self.mock.snapshot(|state| state.fail_setup) {
            return Err(anyhow::anyhow!("mockall app failed: storage setup inspect"));
        }
        Ok(SecretStorageSetupInspection {
            key_exists: false,
            piv_version: PivApplicationVersion::minimum_for_secret_storage(),
            pin_retries: 1,
            manifest_bytes: None,
            occupied_object_ids: Vec::new(),
        })
    }

    fn initialize_secret_storage(
        &mut self,
        _serial: u32,
        _intent: SecretStorageSetupIntent,
    ) -> Result<()> {
        self.mock.hit_event("setup-initialize");
        Ok(())
    }

    fn finalize_secret_storage_setup(
        &mut self,
        _serial: u32,
        _intent: SecretStorageSetupIntent,
    ) -> Result<()> {
        self.mock.hit_event("setup-finalize");
        Ok(())
    }

    fn inspect_secret_storage_write(
        &mut self,
        _serial: u32,
        _storage: &SecretStorageSpec,
    ) -> Result<SecretStorageWriteInspection> {
        Ok(SecretStorageWriteInspection {
            manifest_bytes: if self.mock.snapshot(|state| state.write_manifest_exists) {
                Some(SecretManifest::expected().encode()?)
            } else {
                None
            },
            object_exists: self.mock.snapshot(|state| state.write_object_exists),
        })
    }

    fn store_secret(
        &mut self,
        _serial: u32,
        intent: SecretStorageWriteIntent,
        secret: &SecretMaterial,
    ) -> Result<()> {
        let name = intent.storage.name;
        if self.mock.snapshot(|state| state.fail_on_store == Some(name)) {
            let status = self.mock.snapshot(|state| state.store_failure_status);
            if status == 409 {
                return Err(anyhow::anyhow!("selected YubiKey was already updated"));
            }
            return Err(anyhow::anyhow!("mockall app failed: storage store"));
        }

        let value = secret_bytes(secret)?;
        self.mock.hit_event("store");
        self.mock.configure(|state| {
            state.stores.push(name);
            state.loaded_values.insert(name, value);
        });
        Ok(())
    }

    fn inspect_secret_storage_read(
        &mut self,
        _serial: u32,
        _storage: &SecretStorageSpec,
    ) -> Result<SecretStorageReadInspection> {
        Ok(SecretStorageReadInspection {
            manifest_bytes: Some(SecretManifest::expected().encode()?),
            encoded: Some(vec![1]),
        })
    }

    fn load_secret(
        &mut self,
        _serial: u32,
        intent: &SecretStorageReadIntent,
        _pin: Option<&SecretMaterial>,
    ) -> Result<SecretMaterial> {
        self.mock.hit_event("load");
        let name = intent.storage.name;
        let bytes = self.mock.snapshot(|state| {
            if state.loaded_len == 0 {
                Vec::new()
            } else {
                state
                    .loaded_values
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| vec![0; state.loaded_len])
            }
        });
        Ok(secret_material(bytes))
    }
}

#[derive(Clone)]
struct AppMockState {
    primary_serial: u32,
    spare_serial: u32,
    primary_requires_pin: bool,
    spare_requires_pin: bool,
    primary_available: bool,
    device_resolution_sequence: Vec<u32>,
    rotation_continuations: Vec<bool>,
    loaded_len: usize,
    loaded_values: BTreeMap<SecretName, Vec<u8>>,
    fail_setup: bool,
    fail_on_store: Option<SecretName>,
    store_failure_status: usize,
    write_manifest_exists: bool,
    write_object_exists: bool,
    pin_error: Option<&'static str>,
    stdin_json_error: Option<&'static str>,
    streamed_secret_error: Option<&'static str>,
    secret_values: BTreeMap<SecretName, Vec<u8>>,
    secret_errors: BTreeMap<SecretName, &'static str>,
    output_secret: Option<Vec<u8>>,
    stores: Vec<SecretName>,
    resolution_order: Vec<&'static str>,
    reports: Vec<(u32, CheckStatus)>,
    expected_events: BTreeMap<&'static str, usize>,
    event_hits: BTreeMap<&'static str, usize>,
    event_order: Vec<&'static str>,
}

impl Default for AppMockState {
    fn default() -> Self {
        Self {
            primary_serial: 2001,
            spare_serial: 2002,
            primary_requires_pin: false,
            spare_requires_pin: false,
            primary_available: true,
            device_resolution_sequence: Vec::new(),
            rotation_continuations: Vec::new(),
            loaded_len: 1,
            loaded_values: BTreeMap::new(),
            fail_setup: false,
            fail_on_store: None,
            store_failure_status: 500,
            write_manifest_exists: true,
            write_object_exists: false,
            pin_error: None,
            stdin_json_error: None,
            streamed_secret_error: None,
            secret_values: [
                (SecretName::BwEmail, b"u@example.com".to_vec()),
                (SecretName::BwPassword, b"secret".to_vec()),
                (SecretName::BwsAccessToken, b"token".to_vec()),
            ]
            .into_iter()
            .collect(),
            secret_errors: BTreeMap::new(),
            output_secret: None,
            stores: Vec::new(),
            resolution_order: Vec::new(),
            reports: Vec::new(),
            expected_events: BTreeMap::new(),
            event_hits: BTreeMap::new(),
            event_order: Vec::new(),
        }
    }
}

impl AppMockState {
    fn hit_event(&mut self, event: &'static str) {
        self.event_order.push(event);
        *self.event_hits.entry(event).or_insert(0) += 1;
    }

    fn secret_value(&self, secret: SecretName) -> Vec<u8> {
        self.secret_values
            .get(&secret)
            .cloned()
            .unwrap_or_default()
    }
}

fn secret_material(bytes: Vec<u8>) -> SecretMaterial {
    SecretMaterial::from_backend(bytes, |secret| secret.len(), |secret| Ok(secret.clone()))
}

fn secret_bytes(secret: &SecretMaterial) -> Result<Vec<u8>> {
    secret
        .as_backend::<Vec<u8>>()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("mockall app secret backend is unavailable"))
}

fn verify_summary(serial: u32, local_storage: CheckStatus) -> VerifySummary {
    match local_storage {
        CheckStatus::Ok => VerifySummary::local_storage_verified(serial),
        CheckStatus::Failed => VerifySummary::local_storage_failed(serial),
        CheckStatus::Skipped => VerifySummary::local_storage_verified(serial),
    }
}

fn invalid_input(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into())
}
