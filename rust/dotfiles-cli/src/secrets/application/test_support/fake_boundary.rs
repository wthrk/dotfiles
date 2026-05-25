//! application 層 unit test 用の fake 境界実装。
//!
//! production コードと物理的に分離するため `application/test_support/` 配下に置く。
//! `application.rs` の `#[cfg(test)] mod test_support;` から参照する。

use std::io::Cursor;
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    rc::Rc,
};

use anyhow::bail;
use zeroize::Zeroizing;

use crate::{
    secrets::{
        domain,
        ports::{EnrollmentBytes, SecretsBoundary},
        support::protection::{ProtectedInputBuffer, ProtectedSecret, SecretSession},
        EnrollmentSecretSet,
    },
    Result,
};

use crate::secrets::application::enroll_without_local_verify;
use crate::secrets::{application::summary, ports};

pub(crate) struct FakeBoundary {
    pub(crate) devices: RefCell<VecDeque<FakeDevice>>,
    pub(crate) prompts: RefCell<VecDeque<bool>>,
    pub(crate) stdin_terminal: bool,
    pub(crate) stdout_terminal: bool,
}

impl FakeBoundary {
    pub(crate) fn new(devices: Vec<FakeDevice>) -> Self {
        Self {
            devices: RefCell::new(devices.into()),
            prompts: RefCell::new(VecDeque::new()),
            stdin_terminal: true,
            stdout_terminal: false,
        }
    }

    pub(crate) fn with_prompts(self, prompts: Vec<bool>) -> Self {
        *self.prompts.borrow_mut() = prompts.into();
        self
    }

    pub(crate) fn with_stdin_terminal(mut self, stdin_terminal: bool) -> Self {
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

    fn require_serial(&self, serial: Option<u32>, error_message: &'static str) -> Result<()> {
        if !self.stdin_terminal && serial.is_none() {
            bail!("{}", error_message);
        }
        Ok(())
    }

    fn require_option(&self, enabled: bool, option_name: &'static str) -> Result<()> {
        if !self.stdin_terminal && !enabled {
            bail!("pass {} in non-interactive use", option_name);
        }
        Ok(())
    }

    fn require_stdin_pipe(&self) -> Result<()> {
        if self.stdin_terminal {
            bail!("--stdin requires pipe or redirect input");
        }
        Ok(())
    }

    fn require_stdin_json_pipe(&self, enabled: bool) -> Result<()> {
        if enabled && self.stdin_terminal {
            bail!("--stdin-json requires pipe or redirect input");
        }
        Ok(())
    }

    fn require_stdout_pipe(&self) -> Result<()> {
        if self.stdout_terminal {
            bail!("refusing to write secret to terminal; redirect stdout to a file or pipe");
        }
        Ok(())
    }

    fn read_yubikey_pin_bytes(&self) -> Result<Zeroizing<Vec<u8>>> {
        Ok(Zeroizing::new(b"fake-pin".to_vec()))
    }

    fn read_hidden_bytes(&self, _prompt: &str, _limit: usize) -> Result<Zeroizing<Vec<u8>>> {
        Ok(Zeroizing::new(b"fake-hidden".to_vec()))
    }

    fn read_visible_line_bytes(&self, _prompt: &str, _limit: usize) -> Result<Zeroizing<Vec<u8>>> {
        Ok(Zeroizing::new(b"fake-visible".to_vec()))
    }

    fn read_stdin_bytes(&self, _limit: usize) -> Result<Zeroizing<Vec<u8>>> {
        if self.stdin_terminal {
            bail!("--stdin requires pipe or redirect input");
        }
        Ok(Zeroizing::new(b"fake-stdin".to_vec()))
    }

    fn read_enrollment_json_bytes(
        &self,
        _input_limit: usize,
        _field_limit: usize,
    ) -> Result<EnrollmentBytes> {
        if self.stdin_terminal {
            bail!("--stdin-json requires pipe or redirect input");
        }
        Ok(EnrollmentBytes {
            bw_email: Zeroizing::new(b"user@example.com".to_vec()),
            bw_password: Zeroizing::new(b"password".to_vec()),
            bws_access_token: Zeroizing::new(b"token".to_vec()),
        })
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

    fn prompt_continue_rotation(&self) -> Result<bool> {
        Ok(self.prompts.borrow_mut().pop_front().unwrap_or(false))
    }
}

pub(crate) struct FakeDevice {
    pub(crate) serial: u32,
    pub(crate) state: Rc<RefCell<FakeDeviceState>>,
    pub(crate) pin_error: Option<&'static str>,
}

#[derive(Default)]
pub(crate) struct FakeDeviceState {
    pub(crate) key_exists: bool,
    pub(crate) objects: BTreeMap<domain::PivObjectId, Vec<u8>>,
}

impl FakeDevice {
    pub(crate) fn fresh(serial: u32) -> Self {
        Self {
            serial,
            state: Rc::new(RefCell::new(FakeDeviceState::default())),
            pin_error: None,
        }
    }

    pub(crate) fn fresh_with_state(serial: u32) -> (Self, Rc<RefCell<FakeDeviceState>>) {
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

    pub(crate) fn with_pin_error(mut self, pin_error: &'static str) -> Self {
        self.pin_error = Some(pin_error);
        self
    }

    pub(crate) fn provisioned(serial: u32) -> Result<Self> {
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

    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        Ok(Zeroizing::new(self.wrap_key(wrapped_key)?))
    }
}

pub(crate) fn protected_enrollment_secret_set<'session>(
    memory: &'session SecretSession,
) -> Result<EnrollmentSecretSet<'session>> {
    Ok(EnrollmentSecretSet::new(
        make_fake_secret(b"user@example.com", memory)?,
        make_fake_secret(b"password", memory)?,
        make_fake_secret(b"token", memory)?,
    ))
}

pub(crate) fn make_fake_secret<'session>(
    bytes: &'static [u8],
    memory: &'session SecretSession,
) -> Result<ProtectedSecret<'session>> {
    let input =
        ProtectedInputBuffer::read_from(Cursor::new(bytes), bytes.len(), "too large", memory)?;
    input.into_protected_secret(memory)
}
