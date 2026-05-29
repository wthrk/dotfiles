//! `dotfiles secrets` application 層が外部境界へ要求する port 契約。
//!
//! backend capability ごとの module で契約を分離し、application から必要境界を読めるようにする。

pub mod bw;
pub mod io;
pub mod yubikey;

pub use bw::BwsClientPort;
pub use io::{
    BootstrapSecretDocumentInputPort, PinInputPort, ReportPort, RotationContinuationPort,
    SecretInputPort, SecretOutputPort,
};
pub use yubikey::{
    DevicePinPolicyPort, DeviceSerialPort, SecretStoragePort, SpareDeviceSerialPort,
};

#[cfg(test)]
pub use bw::MockBwsClientPort;
#[cfg(test)]
pub use io::{
    MockBootstrapSecretDocumentInputPort, MockPinInputPort, MockReportPort,
    MockRotationContinuationPort, MockSecretInputPort, MockSecretOutputPort,
};
#[cfg(test)]
pub use yubikey::{
    MockDevicePinPolicyPort, MockDeviceSerialPort, MockSecretStoragePort, MockSpareDeviceSerialPort,
};
