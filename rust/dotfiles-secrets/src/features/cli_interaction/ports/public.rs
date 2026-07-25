//! Cross-feature CLI interaction contracts.
pub(crate) use super::io::{
    BackupUpdateConfirmationPort, BitwardenClientSecretInputPort, BootstrapDocumentInputPort,
    ClockPort, PasswordStoreRemoteInputPort, ReportPort, RotationContinuationPort,
    SecretStorageStatusOutputPort, SshPublicKeyOutputPort,
};
#[cfg(test)]
pub(crate) use super::io::{
    MockBackupUpdateConfirmationPort, MockBitwardenClientSecretInputPort,
    MockBootstrapDocumentInputPort, MockClockPort, MockPasswordStoreRemoteInputPort,
    MockReportPort, MockRotationContinuationPort, MockSecretStorageStatusOutputPort,
    MockSshPublicKeyOutputPort,
};
