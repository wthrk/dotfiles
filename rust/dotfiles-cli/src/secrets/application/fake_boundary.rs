//! application 層 unit test 用の fake 境界実装。
//!
//! production source tree には置けない fake/stub 型をこのファイルに集約する。
//! `application.rs` の `#[cfg(test)] mod fake_boundary;` から参照する。

use std::io::Cursor;
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    rc::Rc,
};

use anyhow::bail;

use crate::{
    secrets::{
        domain,
        ports::SecretsBoundary,
        support::protection::{ProtectedInputBuffer, ProtectedSecret, SecretSession},
        EnrollmentSecretSet,
    },
    Result,
};

use super::{enroll_without_local_verify, ports, summary};

pub(super) struct FakeBoundary {
    pub(super) devices: RefCell<VecDeque<FakeDevice>>,
    pub(super) prompts: RefCell<VecDeque<bool>>,
    pub(super) stdin_terminal: bool,
    pub(super) stdout_terminal: bool,
}

impl FakeBoundary {
    pub(super) fn new(devices: Vec<FakeDevice>) -> Self {
        Self {
            devices: RefCell::new(devices.into()),
            prompts: RefCell::new(VecDeque::new()),
            stdin_terminal: true,
            stdout_terminal: false,
        }
    }

    pub(super) fn with_prompts(self, prompts: Vec<bool>) -> Self {
        *self.prompts.borrow_mut() = prompts.into();
        self
    }

    pub(super) fn with_stdin_terminal(mut self, stdin_terminal: bool) -> Self {
        self.stdin_terminal = stdin_terminal;
        self
    }
}

impl SecretsBoundary for FakeBoundary {
    type Device = FakeDevice;

    fn open_device(&mut self, serial: Option<u32>) -> Result<Self::Device> {
        let mut device = self
            .devices
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("fake device queue is empty"))?;
        if let Some(serial) = serial {
            device.serial = serial;
        }
        Ok(device)
    }

    fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
    ) -> Result<Self::Device> {
        let device = self.open_device(spare_serial)?;
        if primary_serial == Some(device.serial) {
            bail!("primary and spare YubiKey serial must be different");
        }
        Ok(device)
    }

    fn stdin_is_terminal(&self) -> bool {
        self.stdin_terminal
    }

    fn stdout_is_terminal(&self) -> bool {
        self.stdout_terminal
    }

    fn read_yubikey_pin<'session>(
        &self,
        session: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>> {
        make_fake_secret(b"fake-pin", session)
    }

    fn read_hidden_secret<'session>(
        &self,
        _prompt: &str,
        _limit: usize,
        session: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>> {
        make_fake_secret(b"fake-hidden", session)
    }

    fn read_visible_secret_line<'session>(
        &self,
        _prompt: &str,
        _limit: usize,
        session: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>> {
        make_fake_secret(b"fake-visible", session)
    }

    fn read_protected_stdin_secret<'session>(
        &self,
        _limit: usize,
        session: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>> {
        if self.stdin_terminal {
            bail!("--stdin requires pipe or redirect input");
        }
        make_fake_secret(b"fake-stdin", session)
    }

    fn read_protected_enrollment_secret_set<'session>(
        &self,
        _input_limit: usize,
        _field_limit: usize,
        session: &'session SecretSession,
    ) -> Result<EnrollmentSecretSet<'session>> {
        if self.stdin_terminal {
            bail!("--stdin-json requires pipe or redirect input");
        }
        Ok(EnrollmentSecretSet::new(
            make_fake_secret(b"user@example.com", session)?,
            make_fake_secret(b"password", session)?,
            make_fake_secret(b"token", session)?,
        ))
    }

    fn write_secret_to_stdout(&self, _bytes: &[u8]) -> Result<()> {
        if self.stdout_terminal {
            bail!("refusing to write secret to terminal; redirect stdout to a file or pipe");
        }
        Ok(())
    }

    fn write_report(&self, _value: &impl serde::Serialize) -> Result<()> {
        Ok(())
    }

    fn prompt_yes_no(&self, _prompt: &str, _session: &SecretSession) -> Result<bool> {
        Ok(self.prompts.borrow_mut().pop_front().unwrap_or(false))
    }
}

pub(super) struct FakeDevice {
    pub(super) serial: u32,
    pub(super) state: Rc<RefCell<FakeDeviceState>>,
    pub(super) pin_error: Option<&'static str>,
}

#[derive(Default)]
pub(super) struct FakeDeviceState {
    pub(super) key_exists: bool,
    pub(super) objects: BTreeMap<domain::PivObjectId, Vec<u8>>,
}

impl FakeDevice {
    pub(super) fn fresh(serial: u32) -> Self {
        Self {
            serial,
            state: Rc::new(RefCell::new(FakeDeviceState::default())),
            pin_error: None,
        }
    }

    pub(super) fn fresh_with_state(serial: u32) -> (Self, Rc<RefCell<FakeDeviceState>>) {
        let state = Rc::new(RefCell::new(FakeDeviceState::default()));
        (
            Self {
                serial,
                state: Rc::clone(&state),
                pin_error: None,
            },
            state,
        )
    }

    pub(super) fn with_pin_error(mut self, pin_error: &'static str) -> Self {
        self.pin_error = Some(pin_error);
        self
    }

    pub(super) fn provisioned(serial: u32) -> Result<Self> {
        let mut device = Self::fresh(serial);
        let session = SecretSession::start()?;
        let secrets = protected_enrollment_secret_set(&session)?;
        enroll_without_local_verify(
            &mut device,
            summary::YubikeyRole::Primary,
            &secrets,
            &session,
        )?;
        Ok(device)
    }
}

impl ports::SecretDevice for FakeDevice {
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

    fn read_object(&mut self, object_id: domain::PivObjectId) -> Result<Option<Vec<u8>>> {
        Ok(self.state.borrow().objects.get(&object_id).cloned())
    }

    fn write_object(&mut self, object_id: domain::PivObjectId, value: &mut [u8]) -> Result<()> {
        self.state
            .borrow_mut()
            .objects
            .insert(object_id, value.to_vec());
        Ok(())
    }

    fn wrap_key(&mut self, key: &[u8]) -> Result<Vec<u8>> {
        Ok(key.iter().map(|byte| byte ^ 0xa5).collect())
    }

    fn verify_pin(&mut self, _pin: &[u8]) -> Result<()> {
        if let Some(pin_error) = self.pin_error {
            bail!(pin_error);
        }
        Ok(())
    }

    fn requires_pin_input(&self) -> bool {
        true
    }

    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Vec<u8>> {
        self.wrap_key(wrapped_key)
    }
}

pub(super) fn protected_enrollment_secret_set<'session>(
    memory: &'session SecretSession,
) -> Result<EnrollmentSecretSet<'session>> {
    Ok(EnrollmentSecretSet::new(
        make_fake_secret(b"user@example.com", memory)?,
        make_fake_secret(b"password", memory)?,
        make_fake_secret(b"token", memory)?,
    ))
}

pub(super) fn make_fake_secret<'session>(
    bytes: &'static [u8],
    memory: &'session SecretSession,
) -> Result<ProtectedSecret<'session>> {
    let input =
        ProtectedInputBuffer::read_from(Cursor::new(bytes), bytes.len(), "too large", memory)?;
    input.into_protected_secret(memory)
}
