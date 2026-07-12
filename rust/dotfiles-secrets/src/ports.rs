//! `dotfiles secrets` application 層が外部境界へ要求する port 契約の module 境界。
//!
//! port は backend capability ごとに `yubikey`、`bw`、`io` へ分ける。この root は
//! 既存 application の trait import を安定させる再公開だけを担い、契約本文は各 backend module に置く。

pub(crate) mod bw;
pub(crate) mod git;
pub(crate) mod gpg;
pub(crate) mod io;
pub(crate) mod yubikey;

pub(crate) use bw::{BwLoginPort, BwsClientPort};
pub(crate) use git::{GitClonePort, PasswordStorePort};
pub(crate) use gpg::{BackupCipherPort, GpgKeyringPort, SshAgentPort};
pub(crate) use io::{
    BackupUpdateConfirmationPort, BootstrapSecretDocumentInputPort, BwOtpInputPort, ClockPort,
    PasswordStoreRemoteInputPort, PinInputPort, ReportPort, RotationContinuationPort,
    SecretInputPort, SecretOutputPort, SshPublicKeyOutputPort,
};
pub(crate) use yubikey::{
    DevicePinPolicyPort, DeviceSerialPort, GpgRecipientPort, SecretStoragePort,
    SpareDeviceSerialPort, YubiKeyDevicePort,
};

#[cfg(test)]
pub(crate) use bw::{MockBwLoginPort, MockBwsClientPort};
#[cfg(test)]
pub(crate) use git::{MockGitClonePort, MockPasswordStorePort};
#[cfg(test)]
pub(crate) use gpg::{MockBackupCipherPort, MockGpgKeyringPort, MockSshAgentPort};
#[cfg(test)]
pub(crate) use io::{
    MockBackupUpdateConfirmationPort, MockBootstrapSecretDocumentInputPort, MockBwOtpInputPort,
    MockClockPort, MockPasswordStoreRemoteInputPort, MockPinInputPort, MockReportPort,
    MockRotationContinuationPort, MockSecretInputPort, MockSecretOutputPort,
    MockSshPublicKeyOutputPort,
};
#[cfg(test)]
pub(crate) use yubikey::{
    MockDevicePinPolicyPort, MockDeviceSerialPort, MockGpgRecipientPort, MockSecretStoragePort,
    MockSpareDeviceSerialPort, MockYubiKeyDevicePort,
};
