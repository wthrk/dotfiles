//! YubiKey technical backend を recovery port 契約へ接続する adapter。
//!
//! concrete receiver と device/PIV state は `support` が所有する。この source は port trait implementation
//! だけを持つ。

use crate::{
    Result,
    features::yubikey_lifecycle::ports::yubikey::{
        DeviceSerialPort, GpgRecipientPort, SecretStoragePort,
    },
    features::yubikey_lifecycle::support::{
        yubikey_backend::{self, YubikeyDeviceBackend, YubikeyRecipientBackend},
        yubikey_device_serial,
        yubikey_storage::{self, YubikeyStorageBackend},
    },
    features::{
        gpg_backup_recovery::ports::public::{ConnectedYubiKey, EnvelopeRecipient},
        yubikey_lifecycle::domain::{
            piv::{PivDeviceProfile, SecretStorageSpec},
            storage::{
                SecretStorageClearIntent, SecretStorageReadInspection, SecretStorageReadIntent,
                SecretStorageSetupInspection, SecretStorageSetupIntent, SecretStorageSetupProbe,
                SecretStorageStatusInspection, SecretStorageWriteInspection,
                SecretStorageWriteIntent,
            },
        },
    },
    foundation::protection::ProtectedSecret,
};

use crate::features::yubikey_lifecycle::{
    ports::public::piv_pin_input::PivPinInputPort, presentation::piv_pin_input::TerminalPivPinInput,
};

impl PivPinInputPort for TerminalPivPinInput {
    fn read_piv_pin_secret(&self) -> Result<ProtectedSecret> {
        TerminalPivPinInput::read_piv_pin_secret(self)
    }

    fn read_current_piv_pin_secret(&self) -> Result<ProtectedSecret> {
        TerminalPivPinInput::read_current_piv_pin_secret(self)
    }

    fn read_new_piv_pin_confirmation(&self) -> Result<ProtectedSecret> {
        TerminalPivPinInput::read_new_piv_pin_confirmation(self)
    }
}

impl DeviceSerialPort for YubikeyDeviceBackend {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
        yubikey_device_serial::resolve_device_serial(self, requested)
    }

    fn inspect_device_profile(&mut self, serial: u32) -> Result<PivDeviceProfile> {
        yubikey_device_serial::inspect_device_profile(self, serial)
    }
}

impl SecretStoragePort for YubikeyStorageBackend {
    fn begin_piv_pin_setup_preflight(
        &mut self,
        serial: u32,
        current_pin: &ProtectedSecret,
    ) -> Result<()> {
        yubikey_storage::begin_piv_pin_setup_preflight(self, serial, current_pin)
    }

    fn change_piv_pin(
        &mut self,
        serial: u32,
        current_pin: &ProtectedSecret,
        new_pin: &ProtectedSecret,
    ) -> Result<()> {
        yubikey_storage::change_piv_pin(self, serial, current_pin, new_pin)
    }

    fn begin_piv_management_session(&mut self, serial: u32, pin: ProtectedSecret) -> Result<()> {
        yubikey_storage::begin_piv_management_session(self, serial, pin)
    }

    fn begin_next_piv_management_session(
        &mut self,
        serial: u32,
        pin: ProtectedSecret,
    ) -> Result<()> {
        yubikey_storage::begin_next_piv_management_session(self, serial, pin)
    }

    fn inspect_secret_storage_setup(
        &mut self,
        serial: u32,
        probe: &SecretStorageSetupProbe,
    ) -> Result<SecretStorageSetupInspection> {
        yubikey_storage::inspect_secret_storage_setup(self, serial, probe)
    }

    fn initialize_secret_storage(
        &mut self,
        serial: u32,
        intent: SecretStorageSetupIntent,
    ) -> Result<Vec<u8>> {
        yubikey_storage::initialize_secret_storage(self, serial, intent)
    }

    fn finalize_secret_storage_setup(
        &mut self,
        serial: u32,
        manifest_bytes: Vec<u8>,
    ) -> Result<()> {
        yubikey_storage::finalize_secret_storage_setup(self, serial, manifest_bytes)
    }

    fn clear_secret_storage(
        &mut self,
        serial: u32,
        intent: SecretStorageClearIntent,
    ) -> Result<Vec<u8>> {
        yubikey_storage::clear_secret_storage(self, serial, intent)
    }

    fn inspect_secret_storage_write(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageWriteInspection> {
        yubikey_storage::inspect_secret_storage_write(self, serial, storage)
    }

    fn inspect_secret_storage_status(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageStatusInspection> {
        yubikey_storage::inspect_secret_storage_status(self, serial, storage)
    }

    fn store_secret(
        &mut self,
        serial: u32,
        intent: SecretStorageWriteIntent,
        secret: &ProtectedSecret,
    ) -> Result<()> {
        yubikey_storage::store_secret(self, serial, intent, secret)
    }

    fn inspect_secret_storage_read(
        &mut self,
        serial: u32,
        storage: &SecretStorageSpec,
    ) -> Result<SecretStorageReadInspection> {
        yubikey_storage::inspect_secret_storage_read(self, serial, storage)
    }

    fn load_secret(
        &mut self,
        serial: u32,
        intent: &SecretStorageReadIntent,
    ) -> Result<ProtectedSecret> {
        yubikey_storage::load_secret(self, serial, intent)
    }
}

impl GpgRecipientPort for YubikeyRecipientBackend {
    fn resolve_connected_recipient(&mut self, serial: u32) -> Result<ConnectedYubiKey> {
        yubikey_backend::resolve_connected_recipient(self, serial)
    }

    fn wrap_dek_for_recipient(
        &mut self,
        serial: u32,
        dek: &ProtectedSecret,
    ) -> Result<EnvelopeRecipient> {
        yubikey_backend::wrap_dek_for_recipient(self, serial, dek)
    }

    fn unwrap_dek(
        &mut self,
        serial: u32,
        recipient: &EnvelopeRecipient,
    ) -> Result<ProtectedSecret> {
        yubikey_backend::unwrap_dek(self, serial, recipient)
    }
}
