// `secrets-internal-test-stub` feature 専用の mockito stub adapter。
//
// この file は `src/secrets/adapters/piv_io.rs` の test-only bridge からのみ読み込まれる。
// production command path は `SelectedDeviceAdapter` の同一 port 契約を通し、fixture の選択だけを
// xtask internal test 経路（`rust/tests/checks/src/static_checks.rs`）から注入する。

use std::{
    io::{Read, Write},
    net::TcpStream,
};

use anyhow::Context;

use super::{
    DeviceCandidate, PivApplicationVersion, PivObjectId, Result, SecretDeviceIo, SecretMaterial,
    SecretStorageSpec, SelectedDeviceAdapter, SelectedDeviceDiscoveryIo, SelectedSecretDevice,
    material_from_protected, protected_from_material, secret_consumer,
};

const INTERNAL_STUB_ENDPOINT_ENV: &str = "DOTFILES_SECRETS_INTERNAL_STUB_MOCKITO_URL";

#[derive(serde::Deserialize)]
struct StubDeviceWire {
    serial: u32,
    label: String,
}

#[derive(serde::Deserialize)]
struct BoolWire {
    value: bool,
}

#[derive(serde::Deserialize)]
struct U8Wire {
    value: u8,
}

#[derive(serde::Deserialize)]
struct PivVersionWire {
    major: u8,
    minor: u8,
    patch: u8,
}

struct TestStubSecretDevice {
    serial: u32,
    pin_verified: bool,
}

struct StubPinVerifier {
    serial: u32,
}

struct StubSealConsumer {
    serial: u32,
    storage: SecretStorageSpec,
    encoded: Option<Vec<u8>>,
}

/// mockito 経由の internal stub から device 候補を取得し、adapter 境界型へ翻訳する。
fn discover_devices() -> Result<Vec<DeviceCandidate>> {
    let response = stub_http_request("GET", "/devices", &[])?;
    let devices = serde_json::from_slice::<Vec<StubDeviceWire>>(&response)
        .context("failed to decode internal stub device list")?;
    Ok(devices
        .into_iter()
        .map(|device| DeviceCandidate {
            serial: device.serial,
            label: device.label,
        })
        .collect())
}

/// 指定 serial の stub device を開き、`SelectedSecretDevice` 境界へ包んで返す。
fn open_device_by_serial(serial: u32) -> Result<SelectedSecretDevice> {
    let path = format!("/devices/{serial}/open");
    stub_http_request("POST", &path, &[])?;
    Ok(SelectedSecretDevice::new(TestStubSecretDevice {
        serial,
        pin_verified: false,
    }))
}

impl SelectedDeviceDiscoveryIo for SelectedDeviceAdapter {
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>> {
        discover_devices()
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<SelectedSecretDevice> {
        open_device_by_serial(serial)
    }
}

impl SecretDeviceIo for TestStubSecretDevice {
    fn key_exists(&mut self) -> Result<bool> {
        stub_json::<BoolWire>(&format!("/devices/{}/key-exists", self.serial)).map(|wire| wire.value)
    }

    fn piv_application_version(&self) -> PivApplicationVersion {
        let wire = stub_json::<PivVersionWire>(&format!("/devices/{}/piv-version", self.serial))
            .unwrap_or(PivVersionWire {
                major: 5,
                minor: 3,
                patch: 0,
            });
        PivApplicationVersion {
            major: wire.major,
            minor: wire.minor,
            patch: wire.patch,
        }
    }

    fn pin_retries(&mut self) -> Result<u8> {
        stub_json::<U8Wire>(&format!("/devices/{}/pin-retries", self.serial)).map(|wire| wire.value)
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        stub_http_request(
            "POST",
            &format!("/devices/{}/management-auth-preconditions", self.serial),
            &[],
        )
        .map(drop)
    }

    fn generate_key(&mut self) -> Result<()> {
        stub_http_request("POST", &format!("/devices/{}/generate-key", self.serial), &[]).map(drop)
    }

    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>> {
        let path = format!("/devices/{}/objects/{}", self.serial, object_id.value());
        match stub_http_request_with_status("GET", &path, &[])? {
            (200, body) => Ok(Some(body)),
            (404, _) => Ok(None),
            (status, _) => anyhow::bail!("internal stub read_object failed: status={status}"),
        }
    }

    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()> {
        let path = format!("/devices/{}/objects/{}", self.serial, object_id.value());
        stub_http_request("PUT", &path, value).map(drop)
    }

    fn requires_pin_input(&self) -> bool {
        stub_json::<BoolWire>(&format!("/devices/{}/requires-pin", self.serial))
            .map(|wire| wire.value)
            .unwrap_or(false)
    }

    fn verify_pin(&mut self, pin: &SecretMaterial) -> Result<()> {
        secret_consumer::consume(
            protected_from_material(pin)?,
            &mut StubPinVerifier {
                serial: self.serial,
            },
        )?;
        self.pin_verified = true;
        Ok(())
    }

    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        plaintext: &SecretMaterial,
    ) -> Result<Vec<u8>> {
        let mut consumer = StubSealConsumer {
            serial: self.serial,
            storage,
            encoded: None,
        };
        secret_consumer::consume(protected_from_material(plaintext)?, &mut consumer)?;
        consumer
            .encoded
            .context("internal stub seal response missing")
    }

    fn open_from_storage(
        &mut self,
        storage: SecretStorageSpec,
        encoded: &[u8],
    ) -> Result<SecretMaterial> {
        let path = format!(
            "/devices/{}/storage/{}/open",
            self.serial, storage.secret_id
        );
        let plaintext = stub_http_request("POST", &path, encoded)?;
        let session = crate::secrets::support::protection::SecretSession::start()?;
        let buffer =
            crate::secrets::support::protection::buffer::ProtectedInputBuffer::read_line_from(
                std::io::Cursor::new(plaintext),
                16 * 1024,
                &session,
            )?;
        buffer
            .into_protected_secret_line(&session, 16 * 1024, "internal stub secret is too large")
            .map(material_from_protected)
    }
}

impl secret_consumer::SecretConsumer for StubPinVerifier {
    fn consume(&mut self, bytes: &[u8]) -> Result<()> {
        stub_http_request("POST", &format!("/devices/{}/verify-pin", self.serial), bytes).map(drop)
    }
}

impl secret_consumer::SecretConsumer for StubSealConsumer {
    fn consume(&mut self, bytes: &[u8]) -> Result<()> {
        let path = format!(
            "/devices/{}/storage/{}/seal",
            self.serial, self.storage.secret_id
        );
        self.encoded = Some(stub_http_request("POST", &path, bytes)?);
        Ok(())
    }
}

fn endpoint() -> Result<String> {
    std::env::var(INTERNAL_STUB_ENDPOINT_ENV)
        .context("internal mockito YubiKey stub endpoint is not configured")
}

fn stub_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T> {
    let body = stub_http_request("GET", path, &[])?;
    serde_json::from_slice(&body)
        .with_context(|| format!("failed to decode internal stub response for {path}"))
}

fn stub_http_request(method: &str, path: &str, body: &[u8]) -> Result<Vec<u8>> {
    let (status, body) = stub_http_request_with_status(method, path, body)?;
    if (200..300).contains(&status) {
        Ok(body)
    } else {
        anyhow::bail!("internal stub request failed: {method} {path} status={status}");
    }
}

fn stub_http_request_with_status(method: &str, path: &str, body: &[u8]) -> Result<(u16, Vec<u8>)> {
    let endpoint = endpoint()?;
    let target = endpoint
        .strip_prefix("http://")
        .ok_or_else(|| anyhow::anyhow!("internal stub endpoint must use http://"))?;
    let (host_port, _) = target.split_once('/').unwrap_or((target, ""));
    let (host, port) = host_port
        .rsplit_once(':')
        .ok_or_else(|| anyhow::anyhow!("internal stub endpoint must include a port"))?;
    let mut stream = TcpStream::connect((host, port.parse::<u16>()?))?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {host_port}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| anyhow::anyhow!("internal stub returned invalid HTTP response"))?;
    let headers = String::from_utf8_lossy(&response[..header_end]);
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| anyhow::anyhow!("internal stub returned missing HTTP status"))?
        .parse::<u16>()?;
    Ok((status, response[header_end + 4..].to_vec()))
}
