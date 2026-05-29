//! `dotfiles secrets` application 層が外部境界へ要求する port 契約。
//!
//! backend ごとの capability module へ分け、application から見える要求先を明確にする。
//! この module は capability 契約と境界データのみを再公開し、処理手順や変換規則は持たない。

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
