//! `secrets-test-stub` feature の YubiKey device stub。
//!
//! PIV device port だけを in-memory 実装へ差し替え、stdin/stdout/stderr と secret 入力順序は
//! application の通常境界に従う。

use std::collections::BTreeMap;

use anyhow::{bail, Context};
use clap::{Parser, ValueEnum};

use crate::secrets::{
    domain::{self, SecretBlob, SecretManifest, SecretName},
    ports::SecretDevice,
    support::{
        blob_crypto::{decrypt_secret_payload, encrypt_secret_payload},
        protection::{ProtectedSecret, SecretSession},
    },
};
use crate::Result;
use dotfiles_cli_secrets_test_contract::{
    CORRUPT_SECRET_ENV, PRIMARY_SERIAL, PRIMARY_STUB_STATE_ENV, READ_PIN_FROM_TTY_ENV,
    SEED_BWS_ACCESS_TOKEN_ENV, SEED_BW_EMAIL_ENV, SEED_BW_PASSWORD_ENV, SPARE_SERIAL,
    SPARE_STUB_STATE_ENV, STUB_STATE_ENV, WRITE_EVENT_PREFIX,
};

const DEFAULT_SERIAL: u32 = PRIMARY_SERIAL;

/// device stub の初期状態と保存後検証条件を integration test contract から受け取る。
///
/// すべて clap の env 解決を通すため、binary 起動時の引数構造をテスト専用に増やさない。
#[derive(Clone, Default, Parser)]
#[command(name = "dotfiles-secrets-test-stub")]
struct TestStubConfig {
    /// serial 個別指定がない device に適用する初期状態。
    #[arg(long, env = STUB_STATE_ENV, value_enum)]
    state: Option<TestDeviceState>,
    /// primary serial の device に適用する初期状態。
    #[arg(long, env = PRIMARY_STUB_STATE_ENV, value_enum)]
    state_2001: Option<TestDeviceState>,
    /// spare serial の device に適用する初期状態。
    #[arg(long, env = SPARE_STUB_STATE_ENV, value_enum)]
    state_2002: Option<TestDeviceState>,
    /// 読み出し・検証系 command の失敗経路を確認するために破損させる secret object。
    #[arg(long, env = CORRUPT_SECRET_ENV, value_parser = parse_test_stub_secret_name)]
    corrupt_secret: Option<SecretName>,
    /// PIN prompt の TTY 契約を検証する場合に、application の PIN 入力境界を通す。
    #[arg(long, env = READ_PIN_FROM_TTY_ENV)]
    read_pin_from_tty: bool,
    /// `bw-email` の保存済み stub 値。
    #[arg(long, env = SEED_BW_EMAIL_ENV)]
    seed_bw_email: Option<String>,
    /// `bw-password` の保存済み stub 値。
    #[arg(long, env = SEED_BW_PASSWORD_ENV)]
    seed_bw_password: Option<String>,
    /// `bws-access-token` の保存済み stub 値。
    #[arg(long, env = SEED_BWS_ACCESS_TOKEN_ENV)]
    seed_bws_access_token: Option<String>,
}

/// device stub が起動時に持つ PIV object 状態。
#[derive(Clone, Copy, ValueEnum)]
enum TestDeviceState {
    /// PIV key と manifest が未作成の device。
    Fresh,
    /// PIV key と manifest だけが作成済みの device。
    Initialized,
    /// 3 secret がすべて保存済みの device。
    Provisioned,
    /// `bw-email` だけを書き込み対象として空けた device。
    WritableBwEmail,
    /// `bw-password` だけを書き込み対象として空けた device。
    WritableBwPassword,
    /// `bws-access-token` だけを書き込み対象として空けた device。
    WritableBwsAccessToken,
}

impl TestStubConfig {
    /// clap の env 解決を通して、テストプロセスから渡された device mock 条件を読む。
    fn from_env() -> Result<Self> {
        Ok(Self::try_parse_from(["dotfiles-secrets-test-stub"])?)
    }

    /// 保存済み状態を作るとき、clap が env から受けた値で既定値を置き換える。
    fn seed_secret(&self, name: SecretName) -> Vec<u8> {
        let value = match name {
            SecretName::BwEmail => self.seed_bw_email.as_deref(),
            SecretName::BwPassword => self.seed_bw_password.as_deref(),
            SecretName::BwsAccessToken => self.seed_bws_access_token.as_deref(),
        };
        value
            .map(|value| value.as_bytes().to_vec())
            .unwrap_or_else(|| match name {
                SecretName::BwEmail => b"u@example.com".to_vec(),
                SecretName::BwPassword => b"pw".to_vec(),
                SecretName::BwsAccessToken => b"token".to_vec(),
            })
    }

    /// serial 固有設定を優先して device stub の初期状態を決める。
    ///
    /// serial 固有設定がない場合は共通設定、共通設定もない場合は fresh device とする。
    fn state_for_serial(&self, serial: u32) -> TestDeviceState {
        match serial {
            PRIMARY_SERIAL => self.state_2001,
            SPARE_SERIAL => self.state_2002,
            _ => None,
        }
        .or(self.state)
        .unwrap_or(TestDeviceState::Fresh)
    }
}

/// integration test contract から受けた secret 名を domain の閉じた集合へ変換する。
fn parse_test_stub_secret_name(value: &str) -> std::result::Result<SecretName, String> {
    value
        .parse()
        .map_err(|_| format!("unsupported YubiKey secret name: {value}"))
}

/// CLI 統合テスト用の YubiKey device stub を生成する factory。
///
/// 対話的 serial 選択では primary から spare の順に同じ候補列を返し、application の通常
/// device 選択順序を変えない。
pub(crate) struct TestDeviceFactory {
    /// contract から読んだ device 初期状態。
    config: TestStubConfig,
    /// serial 指定なしの対話選択で次に返す serial。
    next_interactive_serial: u32,
}

impl TestDeviceFactory {
    /// integration test contract の環境変数から device stub factory を構築する。
    pub(crate) fn from_env() -> Result<Self> {
        let config = TestStubConfig::from_env()?;
        Ok(Self {
            config,
            next_interactive_serial: DEFAULT_SERIAL,
        })
    }

    /// 通常操作対象の device stub を開く。
    pub(crate) fn open_device(&mut self, serial: Option<u32>) -> Result<TestDevice> {
        let serial = serial.unwrap_or_else(|| {
            let serial = self.next_interactive_serial;
            self.next_interactive_serial = SPARE_SERIAL;
            serial
        });
        let mut device = TestDevice::from_config(serial, &self.config)?;
        device.emit_write_events = true;
        Ok(device)
    }

    /// spare 登録対象の device stub を開く。
    ///
    /// primary と同じ serial は、secret 再保存を始める前に実機 adapter と同じ error で拒否する。
    pub(crate) fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
    ) -> Result<TestDevice> {
        let serial = spare_serial.unwrap_or(SPARE_SERIAL);
        if primary_serial == Some(serial) {
            anyhow::bail!("primary and spare YubiKey serial must be different");
        }

        let mut device = TestDevice::from_config(serial, &self.config)?;
        device.emit_write_events = true;
        Ok(device)
    }
}

/// YubiKey PIV object storage を memory 上で保持する device stub。
///
/// `SecretDevice` port 以外の入力境界は持たず、stdin/stdout/stderr の契約は application の
/// 通常境界に残す。
pub(crate) struct TestDevice {
    /// device 固有 serial。AEAD additional data と summary に使う。
    serial: u32,
    /// PIV key が生成済みかを表す stub 状態。
    key_exists: bool,
    /// PIN prompt と fixture secret を決める contract 設定。
    config: TestStubConfig,
    /// write 後に integration test contract の stderr event を出すかを表す。
    emit_write_events: bool,
    /// PIV object ID ごとの保存済み payload。
    objects: BTreeMap<domain::PivObjectId, Vec<u8>>,
}

impl TestDevice {
    /// contract で指定された初期状態を持つ device stub を構築する。
    fn from_config(serial: u32, config: &TestStubConfig) -> Result<Self> {
        match config.state_for_serial(serial) {
            TestDeviceState::Fresh => Ok(Self::fresh(serial, config.clone())),
            TestDeviceState::Initialized => Self::initialized(serial, config.clone()),
            TestDeviceState::Provisioned => Self::provisioned(serial, config),
            TestDeviceState::WritableBwEmail => {
                Self::writable_for(serial, SecretName::BwEmail, config)
            }
            TestDeviceState::WritableBwPassword => {
                Self::writable_for(serial, SecretName::BwPassword, config)
            }
            TestDeviceState::WritableBwsAccessToken => {
                Self::writable_for(serial, SecretName::BwsAccessToken, config)
            }
        }
    }

    /// PIV key も manifest も存在しない device stub を構築する。
    fn fresh(serial: u32, config: TestStubConfig) -> Self {
        Self {
            serial,
            key_exists: false,
            config,
            emit_write_events: false,
            objects: BTreeMap::new(),
        }
    }

    /// PIV key と manifest が作成済みの device stub を構築する。
    fn initialized(serial: u32, config: TestStubConfig) -> Result<Self> {
        let mut device = Self::fresh(serial, config);
        device.initialize_storage()?;
        Ok(device)
    }

    /// 3 secret がすべて復号可能な device stub を構築する。
    ///
    /// `corrupt_secret` が指定された場合は、指定 object だけを invalid payload に置き換える。
    fn provisioned(serial: u32, config: &TestStubConfig) -> Result<Self> {
        let session = SecretSession::start()?;
        let mut device = Self::initialized(serial, config.clone())?;
        device.write_seed_secret(
            SecretName::BwEmail,
            &config.seed_secret(SecretName::BwEmail),
            &session,
        )?;
        device.write_seed_secret(
            SecretName::BwPassword,
            &config.seed_secret(SecretName::BwPassword),
            &session,
        )?;
        device.write_seed_secret(
            SecretName::BwsAccessToken,
            &config.seed_secret(SecretName::BwsAccessToken),
            &session,
        )?;
        if let Some(name) = config.corrupt_secret {
            device
                .objects
                .insert(name.object_id(), b"not-json".to_vec());
        }
        Ok(device)
    }

    /// 指定 secret だけが未保存の writable device stub を構築する。
    fn writable_for(serial: u32, target: SecretName, config: &TestStubConfig) -> Result<Self> {
        let session = SecretSession::start()?;
        let mut device = Self::initialized(serial, config.clone())?;
        for name in SecretName::iter().filter(|name| *name != target) {
            device.write_seed_secret(name, &config.seed_secret(name), &session)?;
        }
        Ok(device)
    }

    /// setup 済み状態として PIV key flag と manifest object を作成する。
    fn initialize_storage(&mut self) -> Result<()> {
        self.key_exists = true;
        let manifest = serde_json::to_vec(&SecretManifest::expected())?;
        self.objects.insert(domain::PivObjectId::MANIFEST, manifest);
        Ok(())
    }

    /// 保存済み fixture secret を encrypted blob として device object へ入れる。
    ///
    /// application の put use case を呼ばず、device stub の初期状態だけを作る。
    fn write_seed_secret(
        &mut self,
        name: SecretName,
        secret: &[u8],
        session: &SecretSession,
    ) -> Result<()> {
        if secret.is_empty() {
            bail!("{} must not be empty", name);
        }

        let blob = self.encrypt_seed_secret(name, secret, session)?;
        self.objects
            .insert(name.object_id(), blob.encode()?.to_vec());
        Ok(())
    }

    /// fixture secret を実 storage と同じ blob format に暗号化する。
    ///
    /// content key と ciphertext の一時平文は `SecretSession` の保護 buffer に置く。
    fn encrypt_seed_secret(
        &mut self,
        name: SecretName,
        secret: &[u8],
        session: &SecretSession,
    ) -> Result<SecretBlob> {
        let additional_data = name.additional_data(self.serial());
        let (nonce, ciphertext, tag, content_key) =
            encrypt_secret_payload(secret, &additional_data, session)?;
        let wrapped_key = self.wrap_key(&content_key)?;
        Ok(SecretBlob {
            name,
            nonce,
            wrapped_key,
            ciphertext,
            tag,
        })
    }
}

impl SecretDevice for TestDevice {
    fn serial(&self) -> u32 {
        self.serial
    }

    fn key_exists(&mut self) -> Result<bool> {
        Ok(self.key_exists)
    }

    fn check_key_generation_preconditions(&mut self) -> Result<()> {
        Ok(())
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        Ok(())
    }

    fn generate_key(&mut self) -> Result<()> {
        self.key_exists = true;
        Ok(())
    }

    fn read_object(&mut self, object_id: domain::PivObjectId) -> Result<Option<Vec<u8>>> {
        Ok(self.objects.get(&object_id).cloned())
    }

    fn write_object(&mut self, object_id: domain::PivObjectId, value: &mut [u8]) -> Result<()> {
        self.objects.insert(object_id, value.to_vec());
        self.emit_write_event(object_id)?;
        Ok(())
    }

    fn wrap_key(&mut self, key: &[u8]) -> Result<Vec<u8>> {
        Ok(key.iter().map(|byte| byte ^ 0xa5).collect())
    }

    fn verify_pin(&mut self, _pin: &[u8]) -> Result<()> {
        Ok(())
    }

    fn requires_pin_input(&self) -> bool {
        self.config.read_pin_from_tty
    }

    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Vec<u8>> {
        self.wrap_key(wrapped_key)
    }
}

impl TestDevice {
    /// 保存直後に同じ device stub から復号し、integration test contract の write event を出す。
    ///
    /// event は CLI integration test の観測用で、secret stdout の安全判定とは別の stderr 契約にする。
    fn emit_write_event(&mut self, object_id: domain::PivObjectId) -> Result<()> {
        if !self.emit_write_events {
            return Ok(());
        }
        let Some(name) = secret_name_for_object_id(object_id) else {
            return Ok(());
        };
        let session = SecretSession::start()?;
        let secret = self.read_seed_secret(name, &session)?;
        secret.with_secret(|value| {
            eprintln!(
                "{} serial={} name={} value={}",
                WRITE_EVENT_PREFIX,
                self.serial,
                name,
                String::from_utf8_lossy(value)
            );
        });
        Ok(())
    }

    /// 保存済み fixture blob を復号し、保護済み secret として返す。
    ///
    /// write event 生成時も raw plaintext を返さず、`ProtectedSecret` の借用範囲に閉じる。
    fn read_seed_secret<'session>(
        &mut self,
        name: SecretName,
        session: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>> {
        let encoded = self
            .objects
            .get(&name.object_id())
            .with_context(|| format!("{} is not stored on this YubiKey", name))?;
        let blob =
            SecretBlob::decode(encoded).with_context(|| format!("failed to decode {}", name))?;
        if blob.name != name {
            bail!("YubiKey secret blob name does not match requested {}", name);
        }
        let additional_data = blob.name.additional_data(self.serial());
        let unwrapped_key = self.unwrap_key(&blob.wrapped_key)?;
        decrypt_secret_payload(
            &unwrapped_key,
            &blob.nonce,
            &blob.ciphertext,
            &blob.tag,
            &additional_data,
            session,
        )
        .map_err(|_| anyhow::anyhow!("failed to decrypt {}", name))
    }
}

/// PIV object ID が secret object に対応する場合だけ secret 名へ戻す。
fn secret_name_for_object_id(object_id: domain::PivObjectId) -> Option<SecretName> {
    SecretName::iter().find(|name| name.object_id() == object_id)
}
