//! `dotfiles secrets` application 層が外部境界へ要求する port 契約の module 境界。
//!
//! port は backend capability ごとに `yubikey`、`bw`、`io` へ分ける。この root は
//! 既存 application の trait import を安定させる再公開だけを担い、契約本文は各 backend module に置く。

pub(crate) mod bw;
pub(crate) mod io;
pub(crate) mod yubikey;

pub(crate) use bw::BwsClientPort;
pub(crate) use io::{
    BootstrapSecretDocumentInputPort, PinInputPort, ReportPort, RotationContinuationPort,
    SecretInputPort, SecretOutputPort,
};
pub(crate) use yubikey::{
    DevicePinPolicyPort, DeviceSerialPort, SecretStoragePort, SpareDeviceSerialPort,
};

#[cfg(test)]
pub(crate) use bw::MockBwsClientPort;
#[cfg(test)]
pub(crate) use io::{
    MockBootstrapSecretDocumentInputPort, MockPinInputPort, MockReportPort,
    MockRotationContinuationPort, MockSecretInputPort, MockSecretOutputPort,
};
#[cfg(test)]
pub(crate) use yubikey::{
    MockDevicePinPolicyPort, MockDeviceSerialPort, MockSecretStoragePort, MockSpareDeviceSerialPort,
};
