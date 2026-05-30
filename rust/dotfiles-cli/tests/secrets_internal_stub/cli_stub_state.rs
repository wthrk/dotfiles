//! `secrets_cli` から利用する internal file-backed stub state helper。
//!
//! この helper は現行暫定実装の shared state file 互換を維持するために残っている是正対象である。
//! 到達設計は `docs/architecture/hexagonal-implementation-rules.md` と
//! `docs/tasks/secret-recovery/work-items/bitwarden-secrets-manager.md` の規約に従い、tests 側で
//! backend state/schema/helper を保持しない。

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::Context;

pub const PRIMARY_SERIAL: u32 = 2001;
pub const SPARE_SERIAL: u32 = 2002;
const MANIFEST_OBJECT_ID: u32 = 0x005f_ff16;
const BWS_ACCESS_TOKEN_OBJECT_ID: u32 = 0x005f_ff19;
const MANIFEST_BYTES: &[u8] = br#"{"version":1,"app":"dotfiles.secret-recovery"}"#;
static STUB_STATE_FILE_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy)]
pub enum StubState {
    Fresh,
    Initialized,
    Provisioned,
    WritableBwsAccessToken,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StubSecret {
    BwEmail,
    BwPassword,
    BwsAccessToken,
}

#[derive(Clone, Copy)]
pub enum StubFixture {
    State(StubState),
    SeedSecret(StubSecret, &'static str),
    CorruptSecret(StubSecret),
    PrimaryOnly,
    ReadPinFromTty,
}

pub struct CliStubFixture {
    pub state_path: PathBuf,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StubDeviceState {
    key_exists: BTreeMap<u32, bool>,
    objects: BTreeMap<(u32, u32), Vec<u8>>,
    plaintexts: BTreeMap<(u32, u8), Vec<u8>>,
    corrupt: BTreeSet<(u32, u8)>,
    include_spare: bool,
    requires_pin: bool,
    write_events: Vec<String>,
    #[serde(default)]
    bws_projects: BTreeMap<String, String>,
    #[serde(default)]
    bws_project_secrets: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    bws_secret_values: BTreeMap<String, Vec<u8>>,
    #[serde(default)]
    bws_fetch_events: Vec<String>,
}

impl StubSecret {
    fn object_id(self) -> u32 {
        match self {
            Self::BwEmail => 0x005f_ff17,
            Self::BwPassword => 0x005f_ff18,
            Self::BwsAccessToken => BWS_ACCESS_TOKEN_OBJECT_ID,
        }
    }

    fn secret_id(self) -> u8 {
        match self {
            Self::BwEmail => 1,
            Self::BwPassword => 2,
            Self::BwsAccessToken => 3,
        }
    }

    fn default_value(self) -> &'static [u8] {
        match self {
            Self::BwEmail => b"u@example.com",
            Self::BwPassword => b"pw",
            Self::BwsAccessToken => b"token",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::BwEmail => "bw-email",
            Self::BwPassword => "bw-password",
            Self::BwsAccessToken => "bws-access-token",
        }
    }
}

impl StubDeviceState {
    fn new(fixtures: &[StubFixture]) -> Self {
        let mut state = Self::fresh_for_all();
        for fixture in fixtures {
            match *fixture {
                StubFixture::State(stub_state) => state.apply_state(PRIMARY_SERIAL, stub_state),
                StubFixture::SeedSecret(secret, value) => {
                    state.key_exists.insert(PRIMARY_SERIAL, true);
                    state.objects.insert(
                        (PRIMARY_SERIAL, MANIFEST_OBJECT_ID),
                        MANIFEST_BYTES.to_vec(),
                    );
                    state.objects.insert(
                        (PRIMARY_SERIAL, secret.object_id()),
                        encoded_object(secret.secret_id()),
                    );
                    state.plaintexts.insert(
                        (PRIMARY_SERIAL, secret.secret_id()),
                        value.as_bytes().to_vec(),
                    );
                }
                StubFixture::CorruptSecret(secret) => {
                    state.corrupt.insert((PRIMARY_SERIAL, secret.secret_id()));
                }
                StubFixture::PrimaryOnly => state.include_spare = false,
                StubFixture::ReadPinFromTty => state.requires_pin = true,
            }
        }
        state
    }

    fn fresh_for_all() -> Self {
        let mut bws_projects = BTreeMap::new();
        bws_projects.insert(
            "bws-project-id-dotfiles".to_owned(),
            "dotfiles-secret-recovery".to_owned(),
        );
        let mut bws_project_secrets = BTreeMap::new();
        bws_project_secrets.insert(
            "bws-project-id-dotfiles".to_owned(),
            BTreeMap::from([
                (
                    "bws-secret-id-gpg".to_owned(),
                    "gpg-secret-key-backup".to_owned(),
                ),
                (
                    "bws-secret-id-pass".to_owned(),
                    "password-store-remote".to_owned(),
                ),
            ]),
        );
        let bws_secret_values = BTreeMap::from([
            (
                "bws-secret-id-access-token".to_owned(),
                StubSecret::BwsAccessToken.default_value().to_vec(),
            ),
            ("bws-secret-id-gpg".to_owned(), b"gpg-secret".to_vec()),
            (
                "bws-secret-id-pass".to_owned(),
                b"https://example.invalid/repo.git".to_vec(),
            ),
        ]);
        let mut state = Self {
            key_exists: BTreeMap::new(),
            objects: BTreeMap::new(),
            plaintexts: BTreeMap::new(),
            corrupt: BTreeSet::new(),
            include_spare: true,
            requires_pin: false,
            write_events: Vec::new(),
            bws_projects,
            bws_project_secrets,
            bws_secret_values,
            bws_fetch_events: Vec::new(),
        };
        state.apply_state(PRIMARY_SERIAL, StubState::Fresh);
        state.apply_state(SPARE_SERIAL, StubState::Fresh);
        state
    }

    fn apply_state(&mut self, serial: u32, state: StubState) {
        self.objects
            .retain(|(object_serial, _), _| *object_serial != serial);
        self.plaintexts
            .retain(|(plain_serial, _), _| *plain_serial != serial);
        match state {
            StubState::Fresh => {
                self.key_exists.insert(serial, false);
            }
            StubState::Initialized => {
                self.key_exists.insert(serial, true);
                self.objects
                    .insert((serial, MANIFEST_OBJECT_ID), MANIFEST_BYTES.to_vec());
            }
            StubState::Provisioned => {
                self.apply_state(serial, StubState::Initialized);
                for secret in [
                    StubSecret::BwEmail,
                    StubSecret::BwPassword,
                    StubSecret::BwsAccessToken,
                ] {
                    self.objects.insert(
                        (serial, secret.object_id()),
                        encoded_object(secret.secret_id()),
                    );
                    self.plaintexts.insert(
                        (serial, secret.secret_id()),
                        secret.default_value().to_vec(),
                    );
                }
            }
            StubState::WritableBwsAccessToken => {
                self.apply_state(serial, StubState::Provisioned);
                self.objects.remove(&(serial, BWS_ACCESS_TOKEN_OBJECT_ID));
                self.plaintexts
                    .remove(&(serial, StubSecret::BwsAccessToken.secret_id()));
            }
        }
    }
}

impl CliStubFixture {
    pub fn new(fixtures: &[StubFixture]) -> Self {
        let state = StubDeviceState::new(fixtures);
        let temp_name = format!(
            "dotfiles-secrets-stub-{}-{}-{}.json",
            std::process::id(),
            STUB_STATE_FILE_SEQ.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        );
        let state_path = std::env::temp_dir().join(temp_name);
        let server = Self { state_path };
        server.store_state(&state).expect("write state");
        server
    }

    pub fn set_serial_state(&self, serial: u32, stub_state: StubState) -> anyhow::Result<()> {
        let mut state = self.load_state()?;
        state.apply_state(serial, stub_state);
        self.store_state(&state)
    }

    pub fn assert_write_event(
        &self,
        serial: u32,
        secret: StubSecret,
        value: &str,
    ) -> anyhow::Result<()> {
        let expected = format_write_event(serial, secret.name(), value);
        let state = self.load_state()?;
        let matched = state.write_events.iter().any(|event| event == &expected);
        let observed_count = state.write_events.len();
        assert!(
            matched,
            "missing write event: serial={serial} name={} redacted={} observed_count={observed_count}",
            secret.name(),
            value == "<redacted>",
        );
        Ok(())
    }

    pub fn assert_stored_secret(
        &self,
        serial: u32,
        secret: StubSecret,
        expected: &str,
    ) -> anyhow::Result<()> {
        let state = self.load_state()?;
        let actual = state
            .plaintexts
            .get(&(serial, secret.secret_id()))
            .cloned()
            .with_context(|| {
                format!(
                    "missing stored secret: serial={serial} name={}",
                    secret.name()
                )
            })?;
        assert!(
            actual == expected.as_bytes(),
            "unexpected stored secret bytes: serial={serial} name={} actual_len={} expected_len={}",
            secret.name(),
            actual.len(),
            expected.len()
        );
        Ok(())
    }

    pub fn assert_bws_secret_value(&self, secret_id: &str, expected: &str) -> anyhow::Result<()> {
        let state = self.load_state()?;
        let actual = state
            .bws_secret_values
            .get(secret_id)
            .cloned()
            .with_context(|| format!("missing bws secret value: id={secret_id}"))?;
        assert!(
            actual == expected.as_bytes(),
            "unexpected bws secret bytes: id={secret_id} actual_len={} expected_len={}",
            actual.len(),
            expected.len()
        );
        Ok(())
    }

    pub fn assert_bws_fetch_event_count(&self, expected_count: usize) -> anyhow::Result<()> {
        let state = self.load_state()?;
        let actual_count = state.bws_fetch_events.len();
        assert!(
            actual_count == expected_count,
            "unexpected bws fetch event count: actual={actual_count} expected={expected_count}"
        );
        Ok(())
    }

    pub fn assert_bws_fetch_event_for_secret(&self, secret_id: &str) -> anyhow::Result<()> {
        let state = self.load_state()?;
        let prefix = format!("DOTFILES_TEST_BWS_FETCH id={secret_id} bytes=");
        assert!(
            state
                .bws_fetch_events
                .iter()
                .any(|event| event.starts_with(&prefix)),
            "missing bws fetch event: id={secret_id} observed_count={}",
            state.bws_fetch_events.len()
        );
        Ok(())
    }

    fn load_state(&self) -> anyhow::Result<StubDeviceState> {
        let body = fs::read(&self.state_path)?;
        Ok(
            bincode::serde::decode_from_slice(&body, bincode::config::standard())
                .map(|(state, _)| state)?,
        )
    }

    fn store_state(&self, state: &StubDeviceState) -> anyhow::Result<()> {
        fs::write(
            &self.state_path,
            bincode::serde::encode_to_vec(state, bincode::config::standard())?,
        )?;
        Ok(())
    }
}

fn encoded_object(secret_id: u8) -> Vec<u8> {
    format!("encoded-secret-{secret_id}").into_bytes()
}

fn format_write_event(serial: u32, secret_name: &str, value: &str) -> String {
    format!("DOTFILES_TEST_STUB_WRITE serial={serial} name={secret_name} value={value}")
}
