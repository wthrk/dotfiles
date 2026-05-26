use std::{cell::RefCell, collections::BTreeMap, env, rc::Rc};

use crate::{
    Result,
    secrets::{
        domain::{
            blob::{CONTENT_KEY_LEN, NONCE_LEN, SecretBlob, TAG_LEN},
            manifest::SecretManifest,
            material::SecretMaterial,
            piv::{PIV_PIN_MAX_LEN, PIV_PIN_MIN_LEN, PivObjectId, SecretName, StorageObjectIds},
            values::DeviceCandidate,
        },
        ports::{DeviceSelectionPort, SecretDevice},
    },
};
use anyhow::Context;
use dotfiles_cli_secrets_test_contract::{
    CORRUPT_SECRET_ENV, PRIMARY_SERIAL, PRIMARY_STUB_STATE_ENV, READ_PIN_FROM_TTY_ENV,
    SEED_BW_EMAIL_ENV, SEED_BW_PASSWORD_ENV, SEED_BWS_ACCESS_TOKEN_ENV, SPARE_SERIAL,
    SPARE_STUB_STATE_ENV, STUB_STATE_ENV, StubState, format_write_event,
};

const REDACTED_WRITE_VALUE: &str = "<redacted>";

/// env 契約の state 文字列を `StubState` へ変換する。
///
/// 無効値を panic で止めず `None` へ落とすことで、fixture 未指定時に default state へ戻せるようにする。
fn parse_state_env(key: &str) -> Option<StubState> {
    env::var(key)
        .ok()
        .and_then(|value| StubState::parse_env_value(value.as_str()))
}

/// serial ごとの既定 state を返す。
///
/// primary は既存運用データ読み出し検証が多いため `Provisioned`、spare は登録フロー検証のため `Fresh` を既定にする。
fn default_state_for_serial(serial: u32) -> StubState {
    match serial {
        PRIMARY_SERIAL => StubState::Provisioned,
        SPARE_SERIAL => StubState::Fresh,
        _ => StubState::Fresh,
    }
}

/// `SeedSecret` fixture 未指定時に使う既定シークレット値を返す。
fn seed_value_for(name: SecretName) -> Vec<u8> {
    match name {
        SecretName::BwEmail => env::var(SEED_BW_EMAIL_ENV)
            .unwrap_or_else(|_| "u@example.com".to_owned())
            .into_bytes(),
        SecretName::BwPassword => env::var(SEED_BW_PASSWORD_ENV)
            .unwrap_or_else(|_| "pw".to_owned())
            .into_bytes(),
        SecretName::BwsAccessToken => env::var(SEED_BWS_ACCESS_TOKEN_ENV)
            .unwrap_or_else(|_| "token".to_owned())
            .into_bytes(),
    }
}

/// seed fixture が 1 つでも指定されたかを判定する。
///
/// primary の読み出し検証時に lazy seed を有効化するかどうかの gate として使う。
fn seed_env_present() -> bool {
    env::var(SEED_BW_EMAIL_ENV).is_ok()
        || env::var(SEED_BW_PASSWORD_ENV).is_ok()
        || env::var(SEED_BWS_ACCESS_TOKEN_ENV).is_ok()
}

/// 破損シミュレーション対象の secret 名を env から解決する。
fn corrupt_secret_name() -> Option<SecretName> {
    let value = env::var(CORRUPT_SECRET_ENV).ok()?;
    match value.as_str() {
        "bw-email" => Some(SecretName::BwEmail),
        "bw-password" => Some(SecretName::BwPassword),
        "bws-access-token" => Some(SecretName::BwsAccessToken),
        _ => None,
    }
}

/// `SecretDevice` 契約を in-memory 状態で実装する test 専用 adapter。
pub(crate) struct TestStubDeviceAdapter {
    devices: BTreeMap<u32, Rc<RefCell<TestStubDeviceState>>>,
    read_pin_from_tty: bool,
}

impl TestStubDeviceAdapter {
    /// テスト契約 env を読み取り、serial ごとの in-memory PIV 状態を初期化する。
    ///
    /// 同一 production command path を維持したまま dependency selection だけを切り替えるため、
    /// state 制御はすべて env 契約から受け取る。
    fn production() -> Self {
        let default_state_primary = default_state_for_serial(PRIMARY_SERIAL);
        let default_state_spare = default_state_for_serial(SPARE_SERIAL);
        let default_state = parse_state_env(STUB_STATE_ENV).unwrap_or(default_state_primary);
        let primary_state = parse_state_env(PRIMARY_STUB_STATE_ENV).unwrap_or(default_state);
        let spare_state = parse_state_env(SPARE_STUB_STATE_ENV).unwrap_or(default_state_spare);

        Self {
            devices: BTreeMap::from([
                (
                    PRIMARY_SERIAL,
                    Rc::new(RefCell::new(TestStubDeviceState::from_state(primary_state))),
                ),
                (
                    SPARE_SERIAL,
                    Rc::new(RefCell::new(TestStubDeviceState::from_state(spare_state))),
                ),
            ]),
            read_pin_from_tty: env::var(READ_PIN_FROM_TTY_ENV).as_deref() == Ok("true"),
        }
    }
}

impl Default for TestStubDeviceAdapter {
    fn default() -> Self {
        Self::production()
    }
}

impl DeviceSelectionPort for TestStubDeviceAdapter {
    type Device = TestStubSecretDevice;
    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>> {
        Ok(self
            .devices
            .keys()
            .map(|serial| DeviceCandidate {
                serial: *serial,
                label: format!("stub-yubikey-{serial}"),
            })
            .collect())
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<Self::Device> {
        let state = self
            .devices
            .entry(serial)
            .or_insert_with(|| Rc::new(RefCell::new(TestStubDeviceState::fresh())))
            .clone();
        Ok(TestStubSecretDevice {
            serial,
            state,
            read_pin_from_tty: self.read_pin_from_tty,
        })
    }
}

/// stub state を保持しつつ `SecretDevice` 振る舞いを提供する仮想デバイス。
pub(crate) struct TestStubSecretDevice {
    serial: u32,
    state: Rc<RefCell<TestStubDeviceState>>,
    read_pin_from_tty: bool,
}

/// same-route 維持のため、real/stub を同一 `SecretDevice` 契約で包む合成デバイス。
///
/// caller はこの enum を分岐根拠に使わず、port 契約経由でのみ操作する責務を負う。
pub(crate) enum SelectedSecretDevice {
    Real(crate::secrets::adapters::yubikey::YubikeySecretDevice),
    Stub(TestStubSecretDevice),
}

/// serial ごとの object map と初期化状態を保持する stub 内部状態。
#[derive(Clone)]
struct TestStubDeviceState {
    key_exists: bool,
    objects: BTreeMap<PivObjectId, Vec<u8>>,
}

impl TestStubDeviceState {
    fn from_state(state: StubState) -> Self {
        match state {
            StubState::Fresh => Self::fresh(),
            StubState::Initialized => Self::initialized(),
            StubState::Provisioned => Self::provisioned(),
            StubState::WritableBwsAccessToken => Self::writable_bws_access_token(),
        }
    }

    fn fresh() -> Self {
        Self {
            key_exists: false,
            objects: BTreeMap::new(),
        }
    }

    fn initialized() -> Self {
        let mut state = Self::fresh();
        state.key_exists = true;
        if let Ok(bytes) = SecretManifest::expected().encode() {
            state.objects.insert(PivObjectId::MANIFEST, bytes);
        }
        state
    }

    fn provisioned() -> Self {
        let mut state = Self::initialized();
        state.objects.insert(
            SecretName::BwEmail.object_id(),
            make_seed_blob(SecretName::BwEmail, b"u@example.com"),
        );
        state.objects.insert(
            SecretName::BwPassword.object_id(),
            make_seed_blob(SecretName::BwPassword, b"pw"),
        );
        state.objects.insert(
            SecretName::BwsAccessToken.object_id(),
            make_seed_blob(SecretName::BwsAccessToken, b"token"),
        );
        state
    }

    fn writable_bws_access_token() -> Self {
        let mut state = Self::initialized();
        state
            .objects
            .remove(&SecretName::BwsAccessToken.object_id());
        state
    }
}

/// manifest の存在と整合を検証し、未初期化状態なら呼び出しを失敗させる。
fn ensure_storage_initialized(state: &mut TestStubSecretDevice) -> Result<SecretManifest> {
    let manifest_bytes = state.read_object(PivObjectId::MANIFEST)?;
    SecretManifest::decode_initialized(manifest_bytes.as_deref())
}

/// 平文 seed を最小 blob 形式へ変換し、stub object map の初期値として保存する。
fn make_seed_blob(name: SecretName, plaintext: &[u8]) -> Vec<u8> {
    let blob = SecretBlob {
        name,
        nonce: [0u8; NONCE_LEN],
        wrapped_key: vec![0u8; CONTENT_KEY_LEN],
        ciphertext: plaintext.to_vec(),
        tag: [0u8; TAG_LEN],
    };
    blob.encode().unwrap_or_default()
}

impl SecretDevice for TestStubSecretDevice {
    fn serial(&self) -> u32 {
        self.serial
    }

    fn key_exists(&mut self) -> Result<bool> {
        Ok(self.state.borrow().key_exists)
    }

    fn check_key_generation_preconditions(&mut self) -> Result<()> {
        Ok(())
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        Ok(())
    }

    fn generate_key(&mut self) -> Result<()> {
        self.state.borrow_mut().key_exists = true;
        Ok(())
    }

    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>> {
        Ok(self.state.borrow().objects.get(&object_id).cloned())
    }

    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()> {
        self.state
            .borrow_mut()
            .objects
            .insert(object_id, value.to_vec());
        if let Some(name) = SecretName::iter().find(|name| name.object_id() == object_id) {
            eprintln!(
                "{}",
                format_write_event(self.serial, &name.to_string(), REDACTED_WRITE_VALUE)
            );
        }
        Ok(())
    }

    fn wrap_key(&mut self, key: &SecretMaterial) -> Result<Vec<u8>> {
        Ok(key.as_ref().to_vec())
    }

    fn requires_pin_input(&self) -> bool {
        self.read_pin_from_tty
    }

    /// PIN 検証の境界条件を実機と同じ長さ制約に揃える。
    ///
    /// same-route 検証で stub だけが緩い入力を許可すると分岐網羅が崩れるため、
    /// 最低限の PIN byte 長チェックは adapter 契約として常に適用する。
    fn verify_pin(&mut self, pin: &SecretMaterial) -> Result<()> {
        if !(PIV_PIN_MIN_LEN..=PIV_PIN_MAX_LEN).contains(&pin.len()) {
            anyhow::bail!("YubiKey PIN must be 6 to 8 bytes");
        }
        Ok(())
    }

    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<SecretMaterial> {
        Ok(SecretMaterial::from_vec(wrapped_key.to_vec()))
    }

    /// object map と seed fixture から secret を読み出す。
    ///
    /// setup 未実行失敗・破損データ失敗・lazy seed 成功を real adapter と同じ順序で通すため、
    /// 読み出し前に manifest 検証を必ず挟み、最後に blob decode と空値検証を行う。
    fn load_secret(&mut self, name: SecretName) -> Result<SecretMaterial> {
        // seed fixture 指定時だけ manifest/object を遅延投入し、
        // setup 未実行デバイスの読み出し失敗経路を通常ケースで隠蔽しない。
        if self.serial == PRIMARY_SERIAL && seed_env_present() {
            let mut state = self.state.borrow_mut();
            if !state.objects.contains_key(&PivObjectId::MANIFEST) {
                state.key_exists = true;
                if let Ok(bytes) = SecretManifest::expected().encode() {
                    state.objects.insert(PivObjectId::MANIFEST, bytes);
                }
            }
        }
        ensure_storage_initialized(self)?;
        if corrupt_secret_name() == Some(name) {
            self.state
                .borrow_mut()
                .objects
                .insert(name.object_id(), b"not-json".to_vec());
        }
        if self.state.borrow().objects.get(&name.object_id()).is_none()
            && self.serial == PRIMARY_SERIAL
        {
            let seeded = seed_value_for(name);
            self.state
                .borrow_mut()
                .objects
                .insert(name.object_id(), make_seed_blob(name, seeded.as_slice()));
        }
        // object map 未登録時だけ seed blob を補充し、既存データの検証経路を上書きしない。
        let encoded = self
            .state
            .borrow()
            .objects
            .get(&name.object_id())
            .cloned()
            .with_context(|| format!("{name} is not stored on this YubiKey"))?;
        let blob =
            SecretBlob::decode(&encoded).with_context(|| format!("failed to decode {name}"))?;
        name.ensure_value_non_empty(blob.ciphertext.as_slice())?;
        Ok(SecretMaterial::from_vec(blob.ciphertext))
    }

    /// force 契約と setup 前提を満たした場合だけ secret object を更新する。
    ///
    /// ここでの停止条件を real adapter と一致させることで、`--force` 有無の振る舞い差分が
    /// test 時だけ変形しないようにする。
    fn store_secret(
        &mut self,
        _random: &impl crate::secrets::ports::RandomBytesPort,
        name: SecretName,
        secret: &SecretMaterial,
        force: bool,
    ) -> Result<()> {
        // force 判定と object 上書き契約は実機 adapter と同じ停止条件で揃える。
        name.ensure_value_non_empty(secret.as_ref())?;
        ensure_storage_initialized(self)?;
        self.check_management_auth_preconditions()?;
        if self.read_object(name.object_id())?.is_some() && !force {
            anyhow::bail!("{} already exists; pass --force to replace it", name);
        }
        let mut encoded = make_seed_blob(name, secret.as_ref());
        self.write_object(name.object_id(), &mut encoded)
    }

    fn setup_storage(&mut self) -> Result<()> {
        // setup 前提判定を domain 側の `ensure_setup_allowed` に委譲し、
        // stub 独自ルールが production 契約を上書きしないようにする。
        let key_exists = self.key_exists()?;
        let manifest_bytes = self.read_object(PivObjectId::MANIFEST)?;
        let mut occupied_object_ids = Vec::new();
        for object_id in StorageObjectIds::iter() {
            if self.read_object(object_id)?.is_some() {
                occupied_object_ids.push(object_id);
            }
        }
        SecretManifest::ensure_setup_allowed(
            key_exists,
            manifest_bytes.as_deref(),
            &occupied_object_ids,
        )?;
        self.generate_key()?;
        let mut manifest = SecretManifest::expected().encode()?;
        self.write_object(PivObjectId::MANIFEST, &mut manifest)
    }
    fn verify_required_secrets(&mut self) -> Result<()> {
        for name in SecretName::iter() {
            let secret = self.load_secret(name)?;
            name.ensure_value_non_empty(secret.as_ref())?;
        }
        Ok(())
    }
}

impl SecretDevice for SelectedSecretDevice {
    /// same-route 検証で real/stub どちらでも同じ serial 契約を返す。
    fn serial(&self) -> u32 {
        match self {
            Self::Real(device) => device.serial(),
            Self::Stub(device) => device.serial(),
        }
    }
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
    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>> {
        match self {
            Self::Real(device) => device.read_object(object_id),
            Self::Stub(device) => device.read_object(object_id),
        }
    }
    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()> {
        match self {
            Self::Real(device) => device.write_object(object_id, value),
            Self::Stub(device) => device.write_object(object_id, value),
        }
    }
    fn wrap_key(&mut self, key: &SecretMaterial) -> Result<Vec<u8>> {
        match self {
            Self::Real(device) => device.wrap_key(key),
            Self::Stub(device) => device.wrap_key(key),
        }
    }
    /// same-route 契約の要点として、呼び出し側は variant を意識せず PIN 要否だけを判定する。
    fn requires_pin_input(&self) -> bool {
        match self {
            Self::Real(device) => device.requires_pin_input(),
            Self::Stub(device) => device.requires_pin_input(),
        }
    }
    fn verify_pin(&mut self, pin: &SecretMaterial) -> Result<()> {
        match self {
            Self::Real(device) => device.verify_pin(pin),
            Self::Stub(device) => device.verify_pin(pin),
        }
    }
    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<SecretMaterial> {
        match self {
            Self::Real(device) => device.unwrap_key(wrapped_key),
            Self::Stub(device) => device.unwrap_key(wrapped_key),
        }
    }
    fn setup_storage(&mut self) -> Result<()> {
        match self {
            Self::Real(device) => device.setup_storage(),
            Self::Stub(device) => device.setup_storage(),
        }
    }
    fn store_secret(
        &mut self,
        random: &impl crate::secrets::ports::RandomBytesPort,
        name: SecretName,
        secret: &SecretMaterial,
        force: bool,
    ) -> Result<()> {
        match self {
            Self::Real(device) => device.store_secret(random, name, secret, force),
            Self::Stub(device) => device.store_secret(random, name, secret, force),
        }
    }
    fn load_secret(&mut self, name: SecretName) -> Result<SecretMaterial> {
        match self {
            Self::Real(device) => device.load_secret(name),
            Self::Stub(device) => device.load_secret(name),
        }
    }
    /// 保存済み必須 secret 検証は route 差分を持たず同一の失敗契約へ合流する。
    fn verify_required_secrets(&mut self) -> Result<()> {
        match self {
            Self::Real(device) => device.verify_required_secrets(),
            Self::Stub(device) => device.verify_required_secrets(),
        }
    }
}
