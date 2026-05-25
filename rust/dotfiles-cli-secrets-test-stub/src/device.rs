//! CLI 統合テスト向けの in-memory YubiKey device stub。
//!
//! `SecretDevice` port を in-memory 実装で代替し、実機 YubiKey への接続は行わない。
//! stdin/stdout/stderr と secret 入力順序は production の `RealSecretsBoundary` を通す。
//! この crate の test double 定義は tests 層に閉じ、production binary は参照しない。

use std::collections::BTreeMap;

use aes_gcm::{aead::AeadInPlace, Aes256Gcm, KeyInit};
use anyhow::{bail, Context};
use rand::Rng;
use zeroize::Zeroizing;

use dotfiles_cli::{
    boundary::{EnrollmentBytes, RealSecretsBoundary, SecretDevice, SecretsBoundary},
    domain::{PivObjectId, SecretBlob, SecretManifest, SecretName, CONTENT_KEY_LEN, NONCE_LEN, TAG_LEN},
};
use dotfiles_cli_secrets_test_contract::{
    PRIMARY_SERIAL, SPARE_SERIAL, WRITE_EVENT_PREFIX,
};

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

/// `SecretsBoundary` を real I/O + stub device で組み合わせる adapter。
///
/// stdin/stdout/stderr/PIN 入力は `RealSecretsBoundary` が担い、device の取得だけを
/// in-memory test double で差し替える。
pub struct TestStubBoundary {
    config: TestStubConfig,
    /// 実プロセスの I/O 境界（stdin/stdout/TTY/JSON）を所有する。
    real: RealSecretsBoundary,
    /// 対話選択時に次に返す serial。
    next_interactive_serial: u32,
}

impl TestStubBoundary {
    /// test stub 設定から boundary を構築する。
    pub fn new(config: TestStubConfig) -> Self {
        Self {
            config,
            real: RealSecretsBoundary,
            next_interactive_serial: PRIMARY_SERIAL,
        }
    }

    /// 通常操作対象の device stub を開く。
    fn open_stub_device(
        &mut self,
        serial: Option<u32>,
    ) -> anyhow::Result<TestDevice> {
        let serial = serial.unwrap_or_else(|| {
            let s = self.next_interactive_serial;
            self.next_interactive_serial = SPARE_SERIAL;
            s
        });
        let mut device = TestDevice::from_config(serial, &self.config)?;
        device.emit_write_events = true;
        Ok(device)
    }

    /// spare 登録対象の device stub を開く。
    fn open_stub_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
    ) -> anyhow::Result<TestDevice> {
        let serial = spare_serial.unwrap_or(SPARE_SERIAL);
        if primary_serial == Some(serial) {
            bail!("primary and spare YubiKey serial must be different");
        }
        let mut device = TestDevice::from_config(serial, &self.config)?;
        device.emit_write_events = true;
        Ok(device)
    }
}

impl SecretsBoundary for TestStubBoundary {
    type Device = TestDevice;

    fn open_device(&mut self, serial: Option<u32>) -> anyhow::Result<Self::Device> {
        self.open_stub_device(serial)
    }

    fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
    ) -> anyhow::Result<Self::Device> {
        self.open_stub_spare_device(spare_serial, primary_serial)
    }

    fn require_serial(
        &self,
        serial: Option<u32>,
        error_message: &'static str,
    ) -> anyhow::Result<()> {
        self.real.require_serial(serial, error_message)
    }

    fn require_option(&self, enabled: bool, option_name: &'static str) -> anyhow::Result<()> {
        self.real.require_option(enabled, option_name)
    }

    fn require_stdin_pipe(&self) -> anyhow::Result<()> {
        self.real.require_stdin_pipe()
    }

    fn require_stdin_json_pipe(&self, enabled: bool) -> anyhow::Result<()> {
        self.real.require_stdin_json_pipe(enabled)
    }

    fn require_stdout_pipe(&self) -> anyhow::Result<()> {
        self.real.require_stdout_pipe()
    }

    fn read_yubikey_pin_bytes(&self) -> anyhow::Result<Zeroizing<Vec<u8>>> {
        self.real.read_yubikey_pin_bytes()
    }

    fn read_hidden_bytes(
        &self,
        prompt: &str,
        limit: usize,
    ) -> anyhow::Result<Zeroizing<Vec<u8>>> {
        self.real.read_hidden_bytes(prompt, limit)
    }

    fn read_visible_line_bytes(
        &self,
        prompt: &str,
        limit: usize,
    ) -> anyhow::Result<Zeroizing<Vec<u8>>> {
        self.real.read_visible_line_bytes(prompt, limit)
    }

    fn read_stdin_bytes(&self, limit: usize) -> anyhow::Result<Zeroizing<Vec<u8>>> {
        self.real.read_stdin_bytes(limit)
    }

    fn read_enrollment_json_bytes(
        &self,
        input_limit: usize,
        field_limit: usize,
    ) -> anyhow::Result<EnrollmentBytes> {
        self.real.read_enrollment_json_bytes(input_limit, field_limit)
    }

    fn write_secret_to_stdout(&self, bytes: &[u8]) -> anyhow::Result<()> {
        self.real.write_secret_to_stdout(bytes)
    }

    fn write_report(&self, value: &impl serde::Serialize) -> anyhow::Result<()> {
        self.real.write_report(value)
    }

    fn prompt_continue_rotation(&self) -> anyhow::Result<bool> {
        self.real.prompt_continue_rotation()
    }
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
            device.objects.insert(name.object_id(), b"not-json".to_vec());
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
        let manifest = serde_json::to_vec(&SecretManifest::expected())?;
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

    fn write_object(
        &mut self,
        object_id: PivObjectId,
        value: &mut [u8],
    ) -> anyhow::Result<()> {
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

/// PIV object ID が secret object に対応する場合だけ secret 名へ戻す。
fn secret_name_for_object_id(object_id: PivObjectId) -> Option<SecretName> {
    SecretName::iter().find(|name| name.object_id() == object_id)
}
