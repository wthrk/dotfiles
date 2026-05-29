//! `secrets-internal-test-stub` feature 専用の file-backed YubiKey backend。
//! production build には含めず、state file を backend として読む。

use crate::{
    Result,
    secrets::{
        adapters::yubikey::{
            DeviceCandidate, PivApplicationVersion, PivObjectId, SecretDeviceIo, SecretStorageSpec,
            SelectedSecretDevice,
        },
        support::protection::ProtectedSecret,
    },
};

use super::state::with_state;

const MANIFEST_OBJECT_ID: u32 = 0x005f_ff16;
const BW_EMAIL_OBJECT_ID: u32 = 0x005f_ff17;
const BW_PASSWORD_OBJECT_ID: u32 = 0x005f_ff18;
const BWS_ACCESS_TOKEN_OBJECT_ID: u32 = 0x005f_ff19;
const PRIMARY_SERIAL: u32 = 2001;
const SPARE_SERIAL: u32 = 2002;

struct TestStubSecretDevice {
    serial: u32,
    pin_verified: bool,
}

pub(super) fn discover_devices() -> Result<Vec<DeviceCandidate>> {
    with_state(|state| {
        let mut out = vec![DeviceCandidate {
            serial: PRIMARY_SERIAL,
            label: format!("stub-yubikey-{PRIMARY_SERIAL}"),
        }];
        if state.include_spare {
            out.push(DeviceCandidate {
                serial: SPARE_SERIAL,
                label: format!("stub-yubikey-{SPARE_SERIAL}"),
            });
        }
        Ok(out)
    })
}

pub(super) fn open_device_by_serial(serial: u32) -> Result<SelectedSecretDevice> {
    Ok(SelectedSecretDevice::new(TestStubSecretDevice {
        serial,
        pin_verified: false,
    }))
}

impl SecretDeviceIo for TestStubSecretDevice {
    fn key_exists(&mut self) -> Result<bool> {
        with_state(|state| Ok(state.key_exists.get(&self.serial).copied().unwrap_or(false)))
    }

    fn piv_application_version(&self) -> PivApplicationVersion {
        PivApplicationVersion {
            major: 5,
            minor: 3,
            patch: 0,
        }
    }

    fn pin_retries(&mut self) -> Result<u8> {
        Ok(1)
    }

    fn check_management_auth_preconditions(&mut self) -> Result<()> {
        Ok(())
    }

    fn generate_key(&mut self) -> Result<()> {
        with_state(|state| {
            state.key_exists.insert(self.serial, true);
            Ok(())
        })
    }

    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>> {
        with_state(|state| {
            Ok(state
                .objects
                .get(&(self.serial, object_id.value()))
                .cloned())
        })
    }

    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()> {
        with_state(|state| {
            state
                .objects
                .insert((self.serial, object_id.value()), value.to_vec());
            Ok(())
        })
    }

    fn requires_pin_input(&self) -> bool {
        with_state(|state| Ok(state.requires_pin)).unwrap_or(false)
    }

    fn verify_pin(&mut self, _pin: &ProtectedSecret) -> Result<()> {
        self.pin_verified = true;
        Ok(())
    }

    fn seal_for_storage(
        &mut self,
        storage: SecretStorageSpec,
        plaintext: &ProtectedSecret,
    ) -> Result<Vec<u8>> {
        let bytes = plaintext.to_test_bytes();
        with_state(|state| {
            state.key_exists.insert(self.serial, true);
            state
                .plaintexts
                .insert((self.serial, storage.secret_id), bytes);
            if let Some(secret_name) = secret_name(storage.secret_id) {
                state.write_events.push(format!(
                    "DOTFILES_TEST_STUB_WRITE serial={} name={} value=<redacted>",
                    self.serial, secret_name
                ));
            }
            Ok(encoded_object(storage.secret_id))
        })
    }

    fn open_from_storage(
        &mut self,
        storage: SecretStorageSpec,
        _encoded: &[u8],
    ) -> Result<ProtectedSecret> {
        let plaintext = with_state(|state| {
            if state.corrupt.contains(&(self.serial, storage.secret_id)) {
                let name = secret_name(storage.secret_id).unwrap_or("unknown");
                anyhow::bail!("corrupt {name}");
            }
            state
                .plaintexts
                .get(&(self.serial, storage.secret_id))
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("missing secret"))
        })?;

        let session = crate::secrets::support::protection::SecretSession::start()?;
        let buffer =
            crate::secrets::support::protection::buffer::ProtectedInputBuffer::read_line_from(
                std::io::Cursor::new(plaintext),
                16 * 1024,
                &session,
            )?;
        buffer.into_protected_secret_line(&session, 16 * 1024, "internal stub secret is too large")
    }
}

fn secret_name(secret_id: u8) -> Option<&'static str> {
    match secret_id {
        1 => Some("bw-email"),
        2 => Some("bw-password"),
        3 => Some("bws-access-token"),
        _ => None,
    }
}

fn encoded_object(secret_id: u8) -> Vec<u8> {
    let object_id = match secret_id {
        1 => BW_EMAIL_OBJECT_ID,
        2 => BW_PASSWORD_OBJECT_ID,
        3 => BWS_ACCESS_TOKEN_OBJECT_ID,
        _ => MANIFEST_OBJECT_ID,
    };
    format!("encoded-object-{object_id}").into_bytes()
}
