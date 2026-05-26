//! CLI 統合テスト向けの in-memory YubiKey device stub。
//!
//! `SecretDevice` port を in-memory 実装で代替し、実機 YubiKey への接続は行わない。
//! stdin/stdout/stderr と secret 入力順序はこの crate 内の boundary 実装が担う。
//! この crate の test double 定義は tests 層に閉じ、production binary は参照しない。

use std::{
    cell::Cell,
    collections::BTreeMap,
    io::{self, IsTerminal, Read, Write},
};

use aes_gcm::{Aes256Gcm, KeyInit, aead::AeadInPlace};
use anyhow::{Context, bail};
use rand::Rng;
use zeroize::Zeroizing;

use dotfiles_cli::{
    domain::{
        BootstrapSecretDocument, CONTENT_KEY_LEN, CheckName, CheckStatus, EnrollSummary, NONCE_LEN,
        PivObjectId, SecretBlob, SecretName, TAG_LEN, VerifySummary, YubikeyRole,
        model::MANIFEST_APP,
    },
    ports::{
        BootstrapSecretLoadPort, BootstrapSecretStorePort, DeviceSelectionInputPort,
        DeviceSelectionPort, DeviceSerialPort, PinInputPort, RandomBytesPort, ReportPort,
        SecretDevice, SecretInputPort, SecretLoadPort, SecretOutputPort, SecretStorePort,
        SpareDeviceSerialPort, SpareDeviceWaitPort, StorageSetupPort, StorageVerifyPort,
    },
};
use dotfiles_cli_secrets_test_contract::{PRIMARY_SERIAL, SPARE_SERIAL, WRITE_EVENT_PREFIX};

/// stub binary に渡す device mock 条件の集合体。
///
/// 環境変数から収集して `main.rs` が構築し、`TestStubBoundary` へ渡す。
pub struct TestStubConfig {
    /// serial 個別指定がない device に適用する初期状態。
    pub stub_state: Option<TestDeviceState>,
    /// primary serial（`PRIMARY_SERIAL`）の device に適用する初期状態。
    pub primary_state: Option<TestDeviceState>,
    /// spare serial（`SPARE_SERIAL`）の device に適用する初期状態。
    pub spare_state: Option<TestDeviceState>,
    /// 読み出し・検証系 command の失敗経路確認のために破損させる secret の kebab-case 名。
    pub corrupt_secret: Option<String>,
    /// application の PIN 入力境界を通す場合は `true`。
    pub read_pin_from_tty: bool,
    /// `bw-email` の保存済み seed 値（`None` は既定値を使う）。
    pub seed_bw_email: Option<String>,
    /// `bw-password` の保存済み seed 値。
    pub seed_bw_password: Option<String>,
    /// `bws-access-token` の保存済み seed 値。
    pub seed_bws_access_token: Option<String>,
}

impl TestStubConfig {
    /// serial 固有設定を優先して device stub の初期状態を決める。
    pub fn state_for_serial(&self, serial: u32) -> TestDeviceState {
        match serial {
            PRIMARY_SERIAL => self.primary_state,
            SPARE_SERIAL => self.spare_state,
            _ => None,
        }
        .or(self.stub_state)
        .unwrap_or(TestDeviceState::Fresh)
    }

    /// 保存済み state を作るとき、seed 設定で既定値を置き換える。
    pub fn seed_secret(&self, name: SecretName) -> Vec<u8> {
        let value = match name {
            SecretName::BwEmail => self.seed_bw_email.as_deref(),
            SecretName::BwPassword => self.seed_bw_password.as_deref(),
            SecretName::BwsAccessToken => self.seed_bws_access_token.as_deref(),
        };
        value
            .map(|v| v.as_bytes().to_vec())
            .unwrap_or_else(|| match name {
                SecretName::BwEmail => b"u@example.com".to_vec(),
                SecretName::BwPassword => b"pw".to_vec(),
                SecretName::BwsAccessToken => b"token".to_vec(),
            })
    }
}

/// device stub が起動時に持つ PIV object 状態。
#[derive(Clone, Copy)]
pub enum TestDeviceState {
    /// PIV key と manifest が未作成の device。
    Fresh,
    /// PIV key と manifest だけが作成済みの device。
    Initialized,
    /// 3 secret がすべて保存済みの device。
    Provisioned,
    /// `bws-access-token` だけが書き込み対象として空いている device。
    WritableBwsAccessToken,
}

/// in-memory device と実プロセス I/O を組み合わせる tests 層の境界実装。
///
/// device の取得は test double で差し替え、stdin/stdout/stderr はこの型が直接扱う。
pub struct TestStubBoundary {
    config: TestStubConfig,
    /// 対話選択時に次に返す serial。
    next_interactive_serial: Cell<u32>,
}

pub struct StubDeviceCandidate {
    serial: u32,
}

impl TestStubBoundary {
    /// test stub 設定から boundary を構築する。
    pub fn new(config: TestStubConfig) -> Self {
        Self {
            config,
            next_interactive_serial: Cell::new(PRIMARY_SERIAL),
        }
    }

    /// 通常操作対象の device stub を開く。
    fn open_stub_device(&mut self, serial: Option<u32>) -> anyhow::Result<TestDevice> {
        if serial.is_none() && !io::stdin().is_terminal() {
            bail!("pass --serial in non-interactive use");
        }
        let serial = serial.unwrap_or_else(|| self.next_interactive_serial.get());
        let mut device = TestDevice::from_config(serial, &self.config)?;
        device.emit_write_events = true;
        Ok(device)
    }
}

impl DeviceSelectionPort for TestStubBoundary {
    type Device = TestDevice;
    type DeviceCandidate = StubDeviceCandidate;

    fn discover_devices(&mut self) -> anyhow::Result<Vec<Self::DeviceCandidate>> {
        let serial = self.next_interactive_serial.get();
        Ok(vec![StubDeviceCandidate { serial }])
    }

    fn open_device_by_serial(&mut self, serial: u32) -> anyhow::Result<Self::Device> {
        self.open_stub_device(Some(serial))
    }
}

impl DeviceSelectionInputPort for TestStubBoundary {
    fn choose_device(&self, devices: &[Self::DeviceCandidate]) -> anyhow::Result<u32> {
        let device = devices.first().context("no YubiKey detected")?;
        Ok(device.serial)
    }
}

impl DeviceSerialPort for TestStubBoundary {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> anyhow::Result<u32> {
        match requested {
            Some(serial) => Ok(serial),
            None => {
                let devices = self.discover_devices()?;
                self.choose_device(&devices)
            }
        }
    }
}

impl SpareDeviceSerialPort for TestStubBoundary {
    fn resolve_spare_device_serial(
        &mut self,
        primary_serial: Option<u32>,
        spare_serial: Option<u32>,
    ) -> anyhow::Result<u32> {
        if let Some(serial) = spare_serial {
            if Some(serial) == primary_serial {
                bail!("primary and spare YubiKey serial must be different");
            }
            return Ok(serial);
        }

        loop {
            let devices = self.discover_devices()?;
            let serial = self.choose_device(&devices)?;
            if Some(serial) != primary_serial {
                return Ok(serial);
            }
            self.wait_for_spare_device()?;
        }
    }
}

impl SpareDeviceWaitPort for TestStubBoundary {
    fn wait_for_spare_device(&self) -> anyhow::Result<()> {
        self.next_interactive_serial.set(SPARE_SERIAL);
        Ok(())
    }
}

impl PinInputPort for TestStubBoundary {
    fn read_pin(&self) -> anyhow::Result<Zeroizing<Vec<u8>>> {
        read_hidden_line_bytes("YubiKey PIN: ", 8)
    }
}

impl SecretInputPort for TestStubBoundary {
    fn read_visible_secret(&self, label: &str) -> anyhow::Result<Zeroizing<Vec<u8>>> {
        read_visible_line_bytes(label, 4096)
    }

    fn read_hidden_secret(&self, label: &str) -> anyhow::Result<Zeroizing<Vec<u8>>> {
        read_hidden_line_bytes(label, 4096)
    }

    fn read_stdin_secret(&self) -> anyhow::Result<Zeroizing<Vec<u8>>> {
        read_stdin_bytes(4096)
    }

    fn read_secret_document_noninteractive(&self) -> anyhow::Result<Zeroizing<Vec<u8>>> {
        read_stdin_bytes(4096)
    }

    fn read_bootstrap_secret_document(&self) -> anyhow::Result<BootstrapSecretDocument> {
        let bytes = self.read_secret_document_noninteractive()?;
        let value: serde_json::Value = serde_json::from_slice(bytes.as_ref())
            .context("stdin JSON must be a bootstrap secret document")?;
        let extract = |key: &str| -> anyhow::Result<&str> {
            value
                .get(key)
                .and_then(serde_json::Value::as_str)
                .with_context(|| format!("missing or invalid string field: {key}"))
        };
        BootstrapSecretDocument::from_interactive_secrets(
            extract("bw-email")?.as_bytes(),
            extract("bw-password")?.as_bytes(),
            extract("bws-access-token")?.as_bytes(),
        )
    }
}

impl SecretOutputPort for TestStubBoundary {
    fn write_secret(&self, bytes: &[u8]) -> anyhow::Result<()> {
        if io::stdout().is_terminal() {
            bail!("refusing to write secret to terminal; pipe stdout to another process");
        }
        io::stdout().lock().write_all(bytes)?;
        Ok(())
    }
}

impl ReportPort for TestStubBoundary {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> anyhow::Result<()> {
        serde_json::to_writer(io::stdout().lock(), &EnrollReport::from_summary(summary))?;
        io::stdout().lock().write_all(b"\n")?;
        Ok(())
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> anyhow::Result<()> {
        serde_json::to_writer(io::stdout().lock(), &VerifyReport::from_summary(summary))?;
        io::stdout().lock().write_all(b"\n")?;
        Ok(())
    }

    fn report_primary_enrollment(&self, serial: u32) -> anyhow::Result<()> {
        self.write_enroll_report(&EnrollSummary::primary_completed(serial))
    }

    fn report_spare_enrollment(&self, serial: u32) -> anyhow::Result<()> {
        self.write_enroll_report(&EnrollSummary::spare_completed(serial))
    }

    fn report_local_storage_verified(&self, serial: u32) -> anyhow::Result<()> {
        self.write_verify_report(&VerifySummary::local_storage_verified(serial))
    }

    fn report_local_storage_failed(&self, serial: u32) -> anyhow::Result<()> {
        self.write_verify_report(&VerifySummary::local_storage_failed(serial))
    }

    fn report_external_checks_unavailable(
        &self,
        serial: u32,
        checks: impl IntoIterator<Item = CheckName>,
    ) -> anyhow::Result<()> {
        self.write_verify_report(&VerifySummary::external_checks_unavailable(serial, checks))
    }
}

impl RandomBytesPort for TestStubBoundary {
    fn fill_random_bytes(&self, out: &mut [u8]) -> anyhow::Result<()> {
        rand::rng().fill(out);
        Ok(())
    }
}

impl SecretLoadPort for TestStubBoundary {
    fn load_secret(&mut self, serial: u32, name: SecretName) -> anyhow::Result<Zeroizing<Vec<u8>>> {
        let mut device = self.open_stub_device(Some(serial))?;
        let bytes = device
            .read_object(PivObjectId::MANIFEST)?
            .context("YubiKey secret manifest is missing")?;
        validate_manifest_bytes(&bytes)?;

        let encoded = device
            .read_object(name.object_id())?
            .with_context(|| format!("{name} is not stored on YubiKey"))?;
        let blob = SecretBlob::decode(&encoded)?;
        Ok(Zeroizing::new(device.decrypt_secret(&blob)?))
    }
}

impl SecretStorePort for TestStubBoundary {
    fn store_secret(
        &mut self,
        serial: u32,
        name: SecretName,
        force: bool,
        secret: &[u8],
    ) -> anyhow::Result<()> {
        if secret.is_empty() {
            bail!("{name} must not be empty");
        }
        let mut device = self.open_stub_device(Some(serial))?;
        let bytes = device
            .read_object(PivObjectId::MANIFEST)?
            .context("YubiKey secret manifest is missing")?;
        validate_manifest_bytes(&bytes)?;
        if !force && device.read_object(name.object_id())?.is_some() {
            bail!("{name} is already stored; pass --force to overwrite");
        }
        device.write_seed_secret(name, secret)?;
        Ok(())
    }
}

impl StorageSetupPort for TestStubBoundary {
    fn setup_storage(&mut self, serial: u32) -> anyhow::Result<()> {
        let mut device = self.open_stub_device(Some(serial))?;
        if device.key_exists()? {
            let manifest = device.read_object(PivObjectId::MANIFEST)?;
            if let Some(raw_manifest) = manifest {
                validate_manifest_bytes(&raw_manifest)?;
                bail!("YubiKey secret storage is already initialized");
            }
            bail!("YubiKey PIV slot is already initialized");
        }
        if let Some(object_id) = device.objects.keys().next().copied() {
            bail!("YubiKey PIV object {} already exists", object_id);
        }
        device.generate_key()?;
        device.write_manifest()?;
        Ok(())
    }
}

impl BootstrapSecretLoadPort for TestStubBoundary {
    fn load_bootstrap_secret_document(
        &mut self,
        serial: u32,
    ) -> anyhow::Result<BootstrapSecretDocument> {
        let bw_email = self.load_secret(serial, SecretName::BwEmail)?;
        let bw_password = self.load_secret(serial, SecretName::BwPassword)?;
        let bws_access_token = self.load_secret(serial, SecretName::BwsAccessToken)?;
        BootstrapSecretDocument::from_interactive_secrets(
            bw_email.as_ref(),
            bw_password.as_ref(),
            bws_access_token.as_ref(),
        )
    }
}

impl BootstrapSecretStorePort for TestStubBoundary {
    fn store_bootstrap_secret_document(
        &mut self,
        serial: u32,
        document: &BootstrapSecretDocument,
    ) -> anyhow::Result<()> {
        self.store_secret(
            serial,
            SecretName::BwEmail,
            false,
            document.bw_email.as_bytes(),
        )?;
        self.store_secret(
            serial,
            SecretName::BwPassword,
            false,
            document.bw_password.as_bytes(),
        )?;
        self.store_secret(
            serial,
            SecretName::BwsAccessToken,
            false,
            document.bws_access_token.as_bytes(),
        )?;
        Ok(())
    }
}

impl StorageVerifyPort for TestStubBoundary {
    fn verify_local_storage(&mut self, serial: u32) -> anyhow::Result<()> {
        for name in SecretName::iter() {
            let secret = self.load_secret(serial, name)?;
            if secret.is_empty() {
                bail!("{name} must not be empty");
            }
        }
        Ok(())
    }
}

#[derive(serde::Serialize)]
struct EnrollReport {
    serial: u32,
    role: ReportRole,
    checks: std::collections::BTreeMap<ReportCheckName, ReportCheckStatus>,
}

#[derive(serde::Serialize)]
struct VerifyReport {
    serial: u32,
    checks: std::collections::BTreeMap<ReportCheckName, ReportCheckStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ReportCheckName {
    Setup,
    BwEmail,
    BwPassword,
    BwsAccessToken,
    LocalStorage,
    Bws,
    BwLogin,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum ReportRole {
    Primary,
    Spare,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ReportCheckStatus {
    Ok,
    Failed,
    Skipped,
}

impl From<CheckName> for ReportCheckName {
    fn from(value: CheckName) -> Self {
        match value {
            CheckName::Setup => Self::Setup,
            CheckName::BwEmail => Self::BwEmail,
            CheckName::BwPassword => Self::BwPassword,
            CheckName::BwsAccessToken => Self::BwsAccessToken,
            CheckName::LocalStorage => Self::LocalStorage,
            CheckName::Bws => Self::Bws,
            CheckName::BwLogin => Self::BwLogin,
        }
    }
}

impl From<CheckStatus> for ReportCheckStatus {
    fn from(value: CheckStatus) -> Self {
        match value {
            CheckStatus::Ok => Self::Ok,
            CheckStatus::Failed => Self::Failed,
            CheckStatus::Skipped => Self::Skipped,
        }
    }
}

impl From<YubikeyRole> for ReportRole {
    fn from(value: YubikeyRole) -> Self {
        match value {
            YubikeyRole::Primary => Self::Primary,
            YubikeyRole::Spare => Self::Spare,
        }
    }
}

impl EnrollReport {
    fn from_summary(summary: &EnrollSummary) -> Self {
        Self {
            serial: summary.serial,
            role: summary.role.into(),
            checks: summary
                .checks
                .iter()
                .map(|(name, status)| ((*name).into(), (*status).into()))
                .collect(),
        }
    }
}

impl VerifyReport {
    fn from_summary(summary: &VerifySummary) -> Self {
        Self {
            serial: summary.serial,
            checks: summary
                .checks
                .iter()
                .map(|(name, status)| ((*name).into(), (*status).into()))
                .collect(),
        }
    }
}

fn read_stdin_bytes(limit: usize) -> anyhow::Result<Zeroizing<Vec<u8>>> {
    if io::stdin().is_terminal() {
        bail!("pass --stdin in non-interactive use");
    }
    let mut bytes = Vec::new();
    io::stdin().read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        bail!("secret is too large");
    }
    while bytes.ends_with(b"\n") || bytes.ends_with(b"\r") {
        bytes.pop();
    }
    Ok(Zeroizing::new(bytes))
}

fn read_visible_line_bytes(prompt: &str, limit: usize) -> anyhow::Result<Zeroizing<Vec<u8>>> {
    eprint!("{prompt}");
    io::stderr().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
    if line.len() > limit {
        bail!("secret is too large");
    }
    Ok(Zeroizing::new(line.into_bytes()))
}

fn read_hidden_line_bytes(prompt: &str, limit: usize) -> anyhow::Result<Zeroizing<Vec<u8>>> {
    read_visible_line_bytes(prompt, limit)
}

/// YubiKey PIV object storage を memory 上で保持する device stub。
///
/// `SecretDevice` port のみを実装し、stdin/stdout/stderr の I/O は持たない。
pub struct TestDevice {
    /// device 固有 serial。AEAD additional data と summary に使う。
    serial: u32,
    /// PIV key が生成済みかを表す stub 状態。
    key_exists: bool,
    /// PIN prompt を TTY 経由で読む場合は true。
    read_pin_from_tty: bool,
    /// write 後に integration test contract の stderr event を出すかを表す。
    pub emit_write_events: bool,
    /// PIV object ID ごとの保存済み payload。
    objects: BTreeMap<PivObjectId, Vec<u8>>,
}

impl TestDevice {
    /// contract で指定された初期状態を持つ device stub を構築する。
    pub fn from_config(serial: u32, config: &TestStubConfig) -> anyhow::Result<Self> {
        match config.state_for_serial(serial) {
            TestDeviceState::Fresh => Ok(Self::fresh(serial, config.read_pin_from_tty)),
            TestDeviceState::Initialized => Self::initialized(serial, config),
            TestDeviceState::Provisioned => Self::provisioned(serial, config),
            TestDeviceState::WritableBwsAccessToken => {
                Self::writable_for(serial, SecretName::BwsAccessToken, config)
            }
        }
    }

    /// PIV key も manifest も存在しない device stub を構築する。
    fn fresh(serial: u32, read_pin_from_tty: bool) -> Self {
        Self {
            serial,
            key_exists: false,
            read_pin_from_tty,
            emit_write_events: false,
            objects: BTreeMap::new(),
        }
    }

    /// PIV key と manifest が作成済みの device stub を構築する。
    fn initialized(serial: u32, config: &TestStubConfig) -> anyhow::Result<Self> {
        let mut device = Self::fresh(serial, config.read_pin_from_tty);
        device.initialize_storage()?;
        Ok(device)
    }

    /// 3 secret がすべて復号可能な device stub を構築する。
    fn provisioned(serial: u32, config: &TestStubConfig) -> anyhow::Result<Self> {
        let mut device = Self::initialized(serial, config)?;
        for name in SecretName::iter() {
            device.write_seed_secret(name, &config.seed_secret(name))?;
        }
        if let Some(ref name_str) = config.corrupt_secret {
            let name: SecretName = name_str
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid corrupt_secret name: {name_str}"))?;
            device
                .objects
                .insert(name.object_id(), b"not-json".to_vec());
        }
        Ok(device)
    }

    /// 指定 secret だけが未保存の writable device stub を構築する。
    fn writable_for(
        serial: u32,
        target: SecretName,
        config: &TestStubConfig,
    ) -> anyhow::Result<Self> {
        let mut device = Self::initialized(serial, config)?;
        for name in SecretName::iter().filter(|n| *n != target) {
            device.write_seed_secret(name, &config.seed_secret(name))?;
        }
        Ok(device)
    }

    /// setup 済み状態として PIV key flag と manifest object を作成する。
    fn initialize_storage(&mut self) -> anyhow::Result<()> {
        self.key_exists = true;
        let manifest = expected_manifest_bytes();
        self.objects.insert(PivObjectId::MANIFEST, manifest);
        Ok(())
    }

    /// 保存済み fixture secret を encrypted blob として device object へ入れる。
    fn write_seed_secret(&mut self, name: SecretName, secret: &[u8]) -> anyhow::Result<()> {
        if secret.is_empty() {
            bail!("{} must not be empty", name);
        }
        let blob = self.encrypt_secret(name, secret)?;
        self.objects.insert(name.object_id(), blob.encode()?);
        Ok(())
    }

    /// fixture secret を実 storage と同じ blob format に暗号化する。
    fn encrypt_secret(&mut self, name: SecretName, secret: &[u8]) -> anyhow::Result<SecretBlob> {
        let mut content_key = [0u8; CONTENT_KEY_LEN];
        rand::rng().fill(&mut content_key);
        let nonce: [u8; NONCE_LEN] = rand::random();
        let cipher = Aes256Gcm::new_from_slice(&content_key)
            .map_err(|_| anyhow::anyhow!("invalid AES-256-GCM key length"))?;
        let mut ciphertext = secret.to_vec();
        let tag = cipher
            .encrypt_in_place_detached(
                aes_gcm::Nonce::from_slice(&nonce),
                &name.additional_data(self.serial),
                &mut ciphertext,
            )
            .map_err(|_| anyhow::anyhow!("failed to encrypt seed secret"))?;
        let tag_bytes: [u8; TAG_LEN] = tag
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("failed to extract AES-GCM tag"))?;
        let wrapped_key = self.wrap_key(&content_key)?;
        Ok(SecretBlob {
            name,
            nonce,
            wrapped_key,
            ciphertext,
            tag: tag_bytes,
        })
    }

    /// 保存済み blob を復号し、secret bytes を返す。
    fn decrypt_secret(&mut self, blob: &SecretBlob) -> anyhow::Result<Vec<u8>> {
        let unwrapped_key = self.unwrap_key(&blob.wrapped_key)?;
        if unwrapped_key.len() != CONTENT_KEY_LEN {
            bail!("unwrapped content key has invalid length");
        }
        let cipher = Aes256Gcm::new_from_slice(&unwrapped_key)
            .map_err(|_| anyhow::anyhow!("invalid AES-256-GCM key length"))?;
        let mut plaintext = blob.ciphertext.clone();
        cipher
            .decrypt_in_place_detached(
                aes_gcm::Nonce::from_slice(&blob.nonce),
                &blob.name.additional_data(self.serial),
                &mut plaintext,
                aes_gcm::Tag::from_slice(&blob.tag),
            )
            .map_err(|_| anyhow::anyhow!("failed to decrypt {}", blob.name))?;
        Ok(plaintext)
    }

    /// 保存直後に同じ device stub から復号し、integration test contract の write event を出す。
    ///
    /// event は CLI integration test の観測用で、secret stdout の安全判定とは別の stderr 契約にする。
    fn emit_write_event(&mut self, object_id: PivObjectId) -> anyhow::Result<()> {
        if !self.emit_write_events {
            return Ok(());
        }
        let Some(name) = secret_name_for_object_id(object_id) else {
            return Ok(());
        };
        let encoded = self
            .objects
            .get(&object_id)
            .context("object disappeared after write")?
            .clone();
        let blob = SecretBlob::decode(&encoded)
            .with_context(|| format!("failed to decode {} for write event", name))?;
        let plaintext = self
            .decrypt_secret(&blob)
            .with_context(|| format!("failed to decrypt {} for write event", name))?;
        eprintln!(
            "{} serial={} name={} value={}",
            WRITE_EVENT_PREFIX,
            self.serial,
            name,
            String::from_utf8_lossy(&plaintext)
        );
        Ok(())
    }
}

impl SecretDevice for TestDevice {
    fn serial(&self) -> u32 {
        self.serial
    }

    fn key_exists(&mut self) -> anyhow::Result<bool> {
        Ok(self.key_exists)
    }

    fn check_key_generation_preconditions(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn check_management_auth_preconditions(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn generate_key(&mut self) -> anyhow::Result<()> {
        self.key_exists = true;
        Ok(())
    }

    fn read_object(&mut self, object_id: PivObjectId) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(self.objects.get(&object_id).cloned())
    }

    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> anyhow::Result<()> {
        self.objects.insert(object_id, value.to_vec());
        self.emit_write_event(object_id)?;
        Ok(())
    }

    fn wrap_key(&mut self, key: &[u8]) -> anyhow::Result<Vec<u8>> {
        Ok(key.iter().map(|byte| byte ^ 0xa5).collect())
    }

    fn verify_pin(&mut self, _pin: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }

    fn requires_pin_input(&self) -> bool {
        self.read_pin_from_tty
    }

    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> anyhow::Result<Zeroizing<Vec<u8>>> {
        Ok(Zeroizing::new(self.wrap_key(wrapped_key)?))
    }
}

impl TestDevice {
    fn write_manifest(&mut self) -> anyhow::Result<()> {
        let manifest = expected_manifest_bytes();
        self.objects.insert(PivObjectId::MANIFEST, manifest);
        Ok(())
    }
}

fn expected_manifest_bytes() -> Vec<u8> {
    format!(r#"{{"version":1,"app":"{MANIFEST_APP}"}}"#).into_bytes()
}

fn validate_manifest_bytes(raw_manifest: &[u8]) -> anyhow::Result<()> {
    let value: serde_json::Value = serde_json::from_slice(raw_manifest)?;
    let version = value.get("version").and_then(serde_json::Value::as_u64);
    let app = value.get("app").and_then(serde_json::Value::as_str);
    if version == Some(1) && app == Some(MANIFEST_APP) {
        return Ok(());
    }
    bail!("YubiKey secret manifest does not match dotfiles secret-recovery format")
}

/// PIV object ID が secret object に対応する場合だけ secret 名へ戻す。
fn secret_name_for_object_id(object_id: PivObjectId) -> Option<SecretName> {
    SecretName::iter().find(|name| name.object_id() == object_id)
}
