// application usecase test の port 実行を mockito HTTP route へ集約する共通 support。
//
// この file は `secrets-internal-test-stub` feature の test-only bridge から
// module context へ読み込まれる。mock/fake 本体は `tests/` 配下に置き、production
// build と production command path には含めない。internal test の実行経路は
// `rust/tests/checks/src/static_checks.rs` の `secrets::application` test command に固定する。

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use mockito::{Matcher, Server, ServerGuard};

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
        values::{CheckName, CheckStatus, EnrollSummary, VerifySummary},
    },
    ports::{self, SecretStoragePort},
};

/// usecase port 呼び出しを mockito HTTP route と共有 state へ集約する test support。
///
/// 非 2xx response body は PIN/secret を含み得るため、error へ載せず、path/status から
/// 安全な固定メッセージだけを返す。
pub(crate) struct AppMock {
    server: ServerGuard,
    state: Arc<Mutex<AppMockState>>,
    _get: mockito::Mock,
    _post: mockito::Mock,
}

impl AppMock {
    pub(crate) fn new() -> Self {
        let mut server = Server::new();
        let state = Arc::new(Mutex::new(AppMockState::default()));
        let get_status_state = Arc::clone(&state);
        let get_body_state = Arc::clone(&state);
        let post_status_state = Arc::clone(&state);
        let post_body_state = Arc::clone(&state);

        let get = server
            .mock("GET", Matcher::Any)
            .with_status_code_from_request(move |request| {
                get_status_state
                    .lock()
                    .map(|state| state.get_status(request.path()))
                    .unwrap_or(500)
            })
            .with_body_from_request(move |request| {
                get_body_state
                    .lock()
                    .map(|state| state.get_body(request.path()))
                    .unwrap_or_default()
            })
            .expect_at_least(0)
            .create();
        let post = server
            .mock("POST", Matcher::Any)
            .with_status_code_from_request(move |request| {
                let body = request.body().map(Vec::as_slice).unwrap_or(&[]);
                post_status_state
                    .lock()
                    .map(|mut state| state.post_status(request.path(), body))
                    .unwrap_or(500)
            })
            .with_body_from_request(move |request| {
                let body = request.body().map(Vec::as_slice).unwrap_or(&[]);
                post_body_state
                    .lock()
                    .map(|state| state.post_body(request.path(), body))
                    .unwrap_or_default()
            })
            .expect_at_least(0)
            .create();

        Self {
            server,
            state,
            _get: get,
            _post: post,
        }
    }

    pub(crate) fn expect_event(&mut self, event: &'static str) {
        self.configure(|state| state.expect_event(event, 1));
    }

    pub(crate) fn expect_event_times(&mut self, event: &'static str, hits: usize) {
        self.configure(|state| state.expect_event(event, hits));
    }

    pub(crate) fn set_primary_serial(&self, serial: u32) {
        self.configure(|state| state.primary_serial = serial);
    }

    pub(crate) fn set_spare_serial(&self, serial: u32) {
        self.configure(|state| state.spare_serial = serial);
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

    pub(crate) fn reports(&self) -> Vec<VerifySummary> {
        self.snapshot(|state| {
            state
                .reports
                .iter()
                .map(|(serial, status)| verify_summary(*serial, *status))
                .collect()
        })
    }

    fn request(&self, method: &str, path: &str, body: &[u8]) -> Result<Vec<u8>> {
        let endpoint = self
            .server
            .url()
            .strip_prefix("http://")
            .context("mockito endpoint must be http")?
            .to_string();
        let (host, port) = endpoint
            .rsplit_once(':')
            .context("mockito endpoint must include port")?;
        let mut stream = TcpStream::connect((host, port.parse::<u16>()?))?;
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )?;
        stream.write_all(body)?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response)?;
        let header_end = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .ok_or_else(|| anyhow::anyhow!("mockito response missing header terminator"))?;
        let headers = String::from_utf8_lossy(&response[..header_end]);
        let status = headers
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .ok_or_else(|| anyhow::anyhow!("mockito response missing status"))?
            .parse::<u16>()?;
        let body = response[header_end + 4..].to_vec();
        if (200..300).contains(&status) {
            Ok(body)
        } else {
            anyhow::bail!("{}", safe_error_message(path, status));
        }
    }

    fn configure(&self, update: impl FnOnce(&mut AppMockState)) {
        if let Ok(mut state) = self.state.lock() {
            update(&mut state);
        }
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
                "mockito app event `{event}` hit count mismatch"
            );
        }
    }
}

/// application usecase が要求する port trait を mockito route 経由で実装する境界。
///
/// 独自 fake の field 駆動ではなく、各 port method を `AppMock::request` に集約して
/// usecase からは production と同じ port 契約だけを見せる。
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

    pub(crate) fn expect_store_times(mut self, hits: usize) -> Self {
        self.mock.expect_event_times("store", hits);
        self
    }

    pub(crate) fn expect_report(mut self) -> Self {
        self.mock.expect_event("report");
        self
    }

    pub(crate) fn expect_pin(mut self) -> Self {
        self.mock.expect_event("pin");
        self
    }

    pub(crate) fn expect_enrollment_success(self) -> Self {
        self.expect_setup().expect_store_times(3).expect_report()
    }

    pub(crate) fn expect_rotation_success(self) -> Self {
        self.expect_store_times(1).expect_report()
    }
}

impl ports::DeviceSerialPort for AppMockBoundary {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
        let body = option_u32_body(requested);
        parse_u32(&self.mock.request("POST", "/device/primary/resolve", &body)?)
    }
}

impl ports::SpareDeviceSerialPort for AppMockBoundary {
    fn resolve_spare_device_serial(
        &mut self,
        requested_spare_serial: Option<u32>,
    ) -> Result<u32> {
        let body = option_u32_body(requested_spare_serial);
        parse_u32(&self.mock.request("POST", "/device/spare/resolve", &body)?)
    }
}

impl ports::DevicePinPolicyPort for AppMockBoundary {
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool> {
        parse_bool(
            &self
                .mock
                .request("GET", &format!("/device/{serial}/requires-pin"), &[])?,
        )
    }
}

impl ports::PinInputPort for AppMockBoundary {
    fn read_pin(&self) -> Result<SecretMaterial> {
        let bytes = self.mock.request("POST", "/pin/read", &[])?;
        Ok(secret_material(bytes))
    }
}

impl ports::SecretInputPort for AppMockBoundary {
    fn read_bw_email_secret(&self) -> Result<SecretMaterial> {
        let bytes = self.mock.request("POST", "/secret/bw-email/read", &[])?;
        Ok(secret_material(bytes))
    }

    fn read_bw_password_secret(&self) -> Result<SecretMaterial> {
        let bytes = self.mock.request("POST", "/secret/bw-password/read", &[])?;
        Ok(secret_material(bytes))
    }

    fn read_bws_access_token_secret(&self) -> Result<SecretMaterial> {
        let bytes = self
            .mock
            .request("POST", "/secret/bws-access-token/read", &[])?;
        Ok(secret_material(bytes))
    }

    fn read_streamed_secret(&self) -> Result<SecretMaterial> {
        let bytes = self.mock.request("POST", "/secret/streamed/read", &[])?;
        Ok(secret_material(bytes))
    }
}

impl ports::BootstrapSecretDocumentInputPort for AppMockBoundary {
    fn read_bootstrap_secret_fields(&self) -> Result<BTreeMap<String, SecretMaterial>> {
        let body = self.mock.request("POST", "/bootstrap/read-fields", &[])?;
        let text = String::from_utf8(body).context("mockito bootstrap response must be UTF-8")?;
        text.lines()
            .map(|line| {
                let (key, value) = line
                    .split_once('=')
                    .ok_or_else(|| anyhow::anyhow!("mockito bootstrap field is invalid"))?;
                Ok((key.to_string(), secret_material(value.as_bytes().to_vec())))
            })
            .collect()
    }
}

impl ports::SecretOutputPort for AppMockBoundary {
    fn write_secret(&self, _secret: &SecretMaterial) -> Result<()> {
        let body = secret_bytes(_secret)?;
        self.mock
            .request("POST", "/secret/output/write", &body)
            .map(drop)
    }
}

impl ports::ReportPort for AppMockBoundary {
    fn write_enroll_report(&self, _summary: &EnrollSummary) -> Result<()> {
        self.mock
            .request("POST", "/report/enroll", b"enroll")
            .map(drop)
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        let local_storage = summary
            .checks
            .get(&CheckName::LocalStorage)
            .copied()
            .unwrap_or(CheckStatus::Skipped);
        let body = format!("{} {}", summary.serial, check_status_wire(local_storage));
        self.mock
            .request("POST", "/report/verify", body.as_bytes())
            .map(drop)
    }
}

impl SecretStoragePort for AppMockBoundary {
    fn inspect_secret_storage_setup(
        &mut self,
        _serial: u32,
        _probe: &SecretStorageSetupProbe,
    ) -> Result<SecretStorageSetupInspection> {
        self.mock.request("POST", "/storage/setup/inspect", &[])?;
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
        self.mock
            .request("POST", "/storage/setup/initialize", &[])
            .map(drop)
    }

    fn inspect_secret_storage_write(
        &mut self,
        _serial: u32,
        _storage: &SecretStorageSpec,
    ) -> Result<SecretStorageWriteInspection> {
        self.mock.request("POST", "/storage/write/inspect", &[])?;
        Ok(SecretStorageWriteInspection {
            manifest_bytes: Some(SecretManifest::expected().encode()?),
            object_exists: false,
        })
    }

    fn store_secret(
        &mut self,
        _serial: u32,
        intent: SecretStorageWriteIntent,
        secret: &SecretMaterial,
    ) -> Result<()> {
        let mut body = intent.storage.name.to_string().into_bytes();
        body.push(b'\n');
        body.extend_from_slice(&secret_bytes(secret)?);
        self.mock
            .request("POST", "/storage/store", &body)
            .map(drop)
    }

    fn inspect_secret_storage_read(
        &mut self,
        _serial: u32,
        _storage: &SecretStorageSpec,
    ) -> Result<SecretStorageReadInspection> {
        self.mock.request("POST", "/storage/read/inspect", &[])?;
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
        let bytes = self.mock.request(
            "POST",
            "/storage/load",
            intent.storage.name.to_string().as_bytes(),
        )?;
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
    loaded_len: usize,
    loaded_values: BTreeMap<SecretName, Vec<u8>>,
    fail_setup: bool,
    fail_on_store: Option<SecretName>,
    store_failure_status: usize,
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
}

impl Default for AppMockState {
    fn default() -> Self {
        Self {
            primary_serial: 2001,
            spare_serial: 2002,
            primary_requires_pin: false,
            spare_requires_pin: false,
            primary_available: true,
            loaded_len: 1,
            loaded_values: BTreeMap::new(),
            fail_setup: false,
            fail_on_store: None,
            store_failure_status: 500,
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
        }
    }
}

impl AppMockState {
    fn expect_event(&mut self, event: &'static str, hits: usize) {
        self.expected_events.insert(event, hits);
    }

    fn hit_event(&mut self, event: &'static str) {
        *self.event_hits.entry(event).or_insert(0) += 1;
    }

    fn get_status(&self, path: &str) -> usize {
        if path.starts_with("/device/") && path.ends_with("/requires-pin") {
            200
        } else {
            404
        }
    }

    fn get_body(&self, path: &str) -> Vec<u8> {
        if let Some(serial) = parse_device_requires_pin_path(path) {
            let value = if serial == self.primary_serial {
                self.primary_requires_pin
            } else if serial == self.spare_serial {
                self.spare_requires_pin
            } else {
                false
            };
            return bool_wire(value);
        }
        Vec::new()
    }

    fn post_status(&mut self, path: &str, body: &[u8]) -> usize {
        match path {
            "/device/primary/resolve" => {
                self.resolution_order.push("primary");
                if parse_optional_u32(body).is_some() || self.primary_available {
                    200
                } else {
                    409
                }
            }
            "/device/spare/resolve" => {
                self.resolution_order.push("spare");
                200
            }
            "/pin/read" => {
                self.hit_event("pin");
                status_for_error(self.pin_error)
            }
            "/secret/streamed/read" => status_for_error(self.streamed_secret_error),
            "/bootstrap/read-fields" => status_for_error(self.stdin_json_error),
            "/secret/bw-email/read" => status_for_error(
                self.secret_errors.get(&SecretName::BwEmail).copied(),
            ),
            "/secret/bw-password/read" => status_for_error(
                self.secret_errors.get(&SecretName::BwPassword).copied(),
            ),
            "/secret/bws-access-token/read" => status_for_error(
                self.secret_errors.get(&SecretName::BwsAccessToken).copied(),
            ),
            "/storage/setup/inspect" => {
                self.hit_event("setup");
                if self.fail_setup { 500 } else { 200 }
            }
            "/storage/setup/initialize" => {
                self.hit_event("setup-initialize");
                200
            }
            "/storage/store" => {
                let secret = parse_secret_name(body);
                if self.fail_on_store == secret {
                    self.store_failure_status
                } else {
                    self.hit_event("store");
                    if let Some((secret, value)) = parse_secret_store_body(body) {
                        self.stores.push(secret);
                        self.loaded_values.insert(secret, value);
                    }
                    200
                }
            }
            "/report/enroll" => {
                self.hit_event("report");
                200
            }
            "/report/verify" => {
                self.hit_event("report");
                if let Some((serial, status)) = parse_verify_report(body) {
                    self.reports.push((serial, status));
                }
                200
            }
            "/secret/output/write" => {
                self.output_secret = Some(body.to_vec());
                200
            }
            "/storage/write/inspect"
            | "/storage/read/inspect"
            | "/storage/load" => 200,
            _ => 404,
        }
    }

    fn post_body(&self, path: &str, body: &[u8]) -> Vec<u8> {
        match path {
            "/device/primary/resolve" if parse_optional_u32(body).is_none() && !self.primary_available => {
                b"pass --serial in non-interactive use".to_vec()
            }
            "/device/primary/resolve" => parse_optional_u32(body)
                .unwrap_or(self.primary_serial)
                .to_string()
                .into_bytes(),
            "/device/spare/resolve" => parse_optional_u32(body)
                .unwrap_or(self.spare_serial)
                .to_string()
                .into_bytes(),
            "/pin/read" => error_or_bytes(self.pin_error, b"123456"),
            "/secret/bw-email/read" => self.secret_or_error(SecretName::BwEmail),
            "/secret/bw-password/read" => self.secret_or_error(SecretName::BwPassword),
            "/secret/bws-access-token/read" => self.secret_or_error(SecretName::BwsAccessToken),
            "/secret/streamed/read" => error_or_bytes(self.streamed_secret_error, b"token"),
            "/bootstrap/read-fields" => error_or_bytes(
                self.stdin_json_error,
                b"bw-email=u@example.com\nbw-password=secret\nbws-access-token=secret\n",
            ),
            "/storage/setup/inspect" if self.fail_setup => b"setup failed".to_vec(),
            "/storage/store" if self.fail_on_store == parse_secret_name(body) => b"store failed".to_vec(),
            "/storage/load" if self.loaded_len == 0 => Vec::new(),
            "/storage/load" => parse_secret_name(body)
                .and_then(|secret| self.loaded_values.get(&secret).cloned())
                .unwrap_or_else(|| vec![0; self.loaded_len]),
            _ => Vec::new(),
        }
    }
}

impl AppMockState {
    fn secret_value(&self, secret: SecretName) -> Vec<u8> {
        self.secret_values
            .get(&secret)
            .cloned()
            .unwrap_or_default()
    }

    fn secret_or_error(&self, secret: SecretName) -> Vec<u8> {
        self.secret_errors
            .get(&secret)
            .map(|message| message.as_bytes().to_vec())
            .unwrap_or_else(|| self.secret_value(secret))
    }
}

fn secret_material(bytes: Vec<u8>) -> SecretMaterial {
    SecretMaterial::from_backend(bytes, |secret| secret.len(), |secret| Ok(secret.clone()))
}

fn secret_bytes(secret: &SecretMaterial) -> Result<Vec<u8>> {
    secret
        .as_backend::<Vec<u8>>()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("mockito app secret backend is unavailable"))
}

fn verify_summary(serial: u32, local_storage: CheckStatus) -> VerifySummary {
    match local_storage {
        CheckStatus::Ok => VerifySummary::local_storage_verified(serial),
        CheckStatus::Failed => VerifySummary::local_storage_failed(serial),
        CheckStatus::Skipped => VerifySummary::external_checks_unavailable(serial, []),
    }
}

fn option_u32_body(value: Option<u32>) -> Vec<u8> {
    value.map(|serial| serial.to_string()).unwrap_or_default().into_bytes()
}

fn parse_u32(body: &[u8]) -> Result<u32> {
    String::from_utf8(body.to_vec())?
        .parse::<u32>()
        .map_err(Into::into)
}

fn parse_bool(body: &[u8]) -> Result<bool> {
    Ok(body == b"true")
}

fn bool_wire(value: bool) -> Vec<u8> {
    if value {
        b"true".to_vec()
    } else {
        b"false".to_vec()
    }
}

fn parse_optional_u32(body: &[u8]) -> Option<u32> {
    if body.is_empty() {
        None
    } else {
        String::from_utf8_lossy(body).parse::<u32>().ok()
    }
}

fn parse_device_requires_pin_path(path: &str) -> Option<u32> {
    path.strip_prefix("/device/")
        .and_then(|rest| rest.strip_suffix("/requires-pin"))
        .and_then(|serial| serial.parse::<u32>().ok())
}

fn parse_secret_name(body: &[u8]) -> Option<SecretName> {
    let name = body.split(|byte| *byte == b'\n').next().unwrap_or(body);
    match name {
        b"bw-email" => Some(SecretName::BwEmail),
        b"bw-password" => Some(SecretName::BwPassword),
        b"bws-access-token" => Some(SecretName::BwsAccessToken),
        _ => None,
    }
}

fn parse_secret_store_body(body: &[u8]) -> Option<(SecretName, Vec<u8>)> {
    let split = body.iter().position(|byte| *byte == b'\n')?;
    let (name, rest) = body.split_at(split);
    let value = rest.get(1..)?;
    Some((parse_secret_name(name)?, value.to_vec()))
}

fn parse_verify_report(body: &[u8]) -> Option<(u32, CheckStatus)> {
    let text = String::from_utf8_lossy(body);
    let (serial, status) = text.split_once(' ')?;
    Some((serial.parse().ok()?, parse_check_status(status)?))
}

fn parse_check_status(status: &str) -> Option<CheckStatus> {
    match status {
        "ok" => Some(CheckStatus::Ok),
        "failed" => Some(CheckStatus::Failed),
        "skipped" => Some(CheckStatus::Skipped),
        _ => None,
    }
}

fn check_status_wire(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Ok => "ok",
        CheckStatus::Failed => "failed",
        CheckStatus::Skipped => "skipped",
    }
}

fn safe_error_message(path: &str, status: u16) -> String {
    match path {
        "/device/primary/resolve" if status == 409 => {
            "pass --serial in non-interactive use".to_string()
        }
        "/pin/read" => "pin verification failed".to_string(),
        "/secret/bws-access-token/read" if status == 500 => {
            "pass --stdin in non-interactive use".to_string()
        }
        "/secret/streamed/read" => "--stdin requires pipe or redirect input".to_string(),
        "/bootstrap/read-fields" => "--stdin-json requires pipe or redirect input".to_string(),
        "/storage/setup/inspect" => "mockito app route failed: storage setup inspect".to_string(),
        "/storage/store" if status == 409 => "selected YubiKey was already updated".to_string(),
        "/storage/store" => "mockito app route failed: storage store".to_string(),
        _ => format!("mockito app route failed: path={path} status={status}"),
    }
}

fn status_for_error(error: Option<&str>) -> usize {
    if error.is_some() { 500 } else { 200 }
}

fn error_or_bytes(error: Option<&str>, bytes: &[u8]) -> Vec<u8> {
    error
        .map(|message| message.as_bytes().to_vec())
        .unwrap_or_else(|| bytes.to_vec())
}
