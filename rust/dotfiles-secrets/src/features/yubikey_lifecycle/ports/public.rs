//! Cross-feature capability contracts owned by `yubikey_lifecycle`.

pub(crate) mod diagnostics;
pub(crate) mod piv_pin_input;
pub(crate) use super::yubikey::{DeviceSerialPort, GpgRecipientPort, SecretStoragePort};
#[cfg(test)]
pub(crate) use super::yubikey::{
    MockDeviceSerialPort, MockGpgRecipientPort, MockSecretStoragePort,
};
pub(crate) use crate::features::yubikey_lifecycle::application::{
    run_clear_with::run_clear, run_put::run_put, run_setup_with::run_setup,
    run_status_with::run_status_with,
};
pub(crate) use crate::features::yubikey_lifecycle::domain::{
    commands::{ClearCommand, PutCommand, SetupCommand, StatusCommand},
    manifest::{BOOTSTRAP_SECRET_DOCUMENT_FIELD_LIMIT, BootstrapSecretDocumentInput},
    piv::{SecretName, SecretStorageSpec},
    storage::{SecretStorageReadIntent, SecretStorageStatus, SecretStorageVerificationPlan},
};

pub(crate) fn is_status_invalid_cause(cause: &(dyn std::error::Error + 'static)) -> bool {
    cause.is::<crate::features::yubikey_lifecycle::domain::storage::SecretStorageStatusInvalid>()
}

pub(crate) fn is_uninitialized_cause(cause: &(dyn std::error::Error + 'static)) -> bool {
    cause.is::<crate::features::yubikey_lifecycle::domain::storage::SecretStorageUninitialized>()
}
