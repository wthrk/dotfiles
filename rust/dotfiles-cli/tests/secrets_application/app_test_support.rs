use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;

use anyhow::Context;
use mockito::{Mock, Server, ServerGuard};

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
        values::{EnrollSummary, VerifySummary},
    },
    ports::{self, SecretStoragePort},
};

pub(crate) struct AppMock {
    server: ServerGuard,
    mocks: Vec<Mock>,
}

impl AppMock {
    pub(crate) fn new() -> Self {
        Self {
            server: Server::new(),
            mocks: Vec::new(),
        }
    }

    pub(crate) fn expect_event(&mut self, event: &'static str) {
        let mock = self
            .server
            .mock("POST", format!("/events/{event}").as_str())
            .expect(1)
            .create();
        self.mocks.push(mock);
    }

    pub(crate) fn expect_event_times(&mut self, event: &'static str, hits: usize) {
        let mock = self
            .server
            .mock("POST", format!("/events/{event}").as_str())
            .expect(hits)
            .create();
        self.mocks.push(mock);
    }

    pub(crate) fn event(&self, event: &'static str) -> Result<()> {
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
            "POST /events/{event} HTTP/1.1\r\nHost: {host}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )?;
        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        if !response.starts_with("HTTP/1.1 200") {
            anyhow::bail!("mockito event `{event}` failed: {response}");
        }
        Ok(())
    }
}

pub(crate) struct AppMockBoundary {
    pub(crate) mock: AppMock,
    pub(crate) device_serial: u32,
    pub(crate) spare_serial: u32,
    pub(crate) primary_requires_pin: bool,
    pub(crate) spare_requires_pin: bool,
    pub(crate) device_serial_available: bool,
    pub(crate) spare_serial_available: bool,
    pub(crate) loaded_len: usize,
    pub(crate) stores: Vec<SecretName>,
    pub(crate) resolution_order: Vec<&'static str>,
    pub(crate) fail_setup: bool,
    pub(crate) fail_on_store: Option<SecretName>,
    pub(crate) pin_error: Option<&'static str>,
    pub(crate) stdin_json_error: Option<&'static str>,
    pub(crate) streamed_secret_error: Option<&'static str>,
    pub(crate) reports: RefCell<Vec<VerifySummary>>,
}

impl AppMockBoundary {
    pub(crate) fn new() -> Self {
        Self {
            mock: AppMock::new(),
            device_serial: 2001,
            spare_serial: 2002,
            primary_requires_pin: false,
            spare_requires_pin: false,
            device_serial_available: true,
            spare_serial_available: true,
            loaded_len: 1,
            stores: Vec::new(),
            resolution_order: Vec::new(),
            fail_setup: false,
            fail_on_store: None,
            pin_error: None,
            stdin_json_error: None,
            streamed_secret_error: None,
            reports: RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn expect_setup(mut self) -> Self {
        self.mock.expect_event("setup");
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
        self.resolution_order.push("primary");
        requested
            .or(self.device_serial_available.then_some(self.device_serial))
            .ok_or_else(|| anyhow::anyhow!("pass --serial in non-interactive use"))
    }
}

impl ports::SpareDeviceSerialPort for AppMockBoundary {
    fn resolve_spare_device_serial(
        &mut self,
        requested_spare_serial: Option<u32>,
    ) -> Result<u32> {
        self.resolution_order.push("spare");
        requested_spare_serial
            .or(self.spare_serial_available.then_some(self.spare_serial))
            .ok_or_else(|| anyhow::anyhow!("pass --serial in non-interactive use"))
    }
}

impl ports::DevicePinPolicyPort for AppMockBoundary {
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool> {
        Ok(if serial == self.device_serial {
            self.primary_requires_pin
        } else {
            self.spare_requires_pin
        })
    }
}

impl ports::PinInputPort for AppMockBoundary {
    fn read_pin(&self) -> Result<SecretMaterial> {
        if let Some(error) = self.pin_error {
            anyhow::bail!(error);
        }
        self.mock.event("pin")?;
        Ok(secret_material(b"123456"))
    }
}

impl ports::SecretInputPort for AppMockBoundary {
    fn read_bw_email_secret(&self) -> Result<SecretMaterial> {
        Ok(secret_material(b"u@example.com"))
    }

    fn read_bw_password_secret(&self) -> Result<SecretMaterial> {
        Ok(secret_material(b"secret"))
    }

    fn read_bws_access_token_secret(&self) -> Result<SecretMaterial> {
        Ok(secret_material(b"token"))
    }

    fn read_streamed_secret(&self) -> Result<SecretMaterial> {
        if let Some(error) = self.streamed_secret_error {
            anyhow::bail!(error);
        }
        Ok(secret_material(b"token"))
    }
}

impl ports::BootstrapSecretDocumentInputPort for AppMockBoundary {
    fn read_bootstrap_secret_fields(&self) -> Result<BTreeMap<String, SecretMaterial>> {
        if let Some(error) = self.stdin_json_error {
            anyhow::bail!(error);
        }
        Ok(BTreeMap::from([
            ("bw-email".to_string(), secret_material(b"u@example.com")),
            ("bw-password".to_string(), secret_material(b"secret")),
            ("bws-access-token".to_string(), secret_material(b"secret")),
        ]))
    }
}

impl ports::SecretOutputPort for AppMockBoundary {
    fn write_secret(&self, _secret: &SecretMaterial) -> Result<()> {
        Ok(())
    }
}

impl ports::ReportPort for AppMockBoundary {
    fn write_enroll_report(&self, _summary: &EnrollSummary) -> Result<()> {
        self.mock.event("report")
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        self.reports.borrow_mut().push(summary.clone());
        self.mock.event("report")
    }
}

impl SecretStoragePort for AppMockBoundary {
    fn inspect_secret_storage_setup(
        &mut self,
        _serial: u32,
        _probe: &SecretStorageSetupProbe,
    ) -> Result<SecretStorageSetupInspection> {
        self.mock.event("setup")?;
        if self.fail_setup {
            anyhow::bail!("setup failed");
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
        Ok(())
    }

    fn inspect_secret_storage_write(
        &mut self,
        _serial: u32,
        _storage: &SecretStorageSpec,
    ) -> Result<SecretStorageWriteInspection> {
        Ok(SecretStorageWriteInspection {
            manifest_bytes: Some(SecretManifest::expected().encode()?),
            object_exists: false,
        })
    }

    fn store_secret(
        &mut self,
        _serial: u32,
        intent: SecretStorageWriteIntent,
        _secret: &SecretMaterial,
    ) -> Result<()> {
        let name = intent.storage.name;
        if self.fail_on_store == Some(name) {
            anyhow::bail!("store failed");
        }
        self.mock.event("store")?;
        self.stores.push(name);
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
        _intent: &SecretStorageReadIntent,
        _pin: Option<&SecretMaterial>,
    ) -> Result<SecretMaterial> {
        Ok(secret_with_len(self.loaded_len))
    }
}

fn secret_material(bytes: &'static [u8]) -> SecretMaterial {
    SecretMaterial::from_backend(
        bytes.to_vec(),
        |secret| secret.len(),
        |secret| Ok(secret.clone()),
    )
}

fn secret_with_len(len: usize) -> SecretMaterial {
    SecretMaterial::from_backend(vec![0; len], |secret| secret.len(), |secret| {
        Ok(secret.clone())
    })
}
