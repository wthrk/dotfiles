//! secrets adapter 層の公開境界。
//!
//! adapter 下位 module をそのまま露出せず、entrypoint が使う runtime adapter 生成だけを提供する。

mod piv_io;
mod yubikey;

use crate::{
    Result,
    secrets::{
        domain::{
            manifest::BootstrapSecretDocument,
            material::SecretMaterial,
            piv::SecretName,
            values::{EnrollSummary, VerifySummary},
        },
        ports::{
            BootstrapSecretDocumentInputPort, DeviceCandidate, DevicePinPolicyPort,
            DeviceSelectionPort, DeviceSerialPort, PinInputPort, ReportPort, SecretInputPort,
            SecretOutputPort, SpareDeviceSerialPort,
        },
    },
};

/// CLI entrypoint が利用する secrets runtime adapter。
///
/// 公開面は port trait 実装型としてのこの型に限定し、下位 adapter module や
/// factory/helper 関数を crate 公開しない。
pub(crate) struct SecretsAdapters {
    boundary: piv_io::RealSecretsBoundary,
}

impl Default for SecretsAdapters {
    fn default() -> Self {
        Self {
            boundary: piv_io::RealSecretsBoundary::default(),
        }
    }
}

impl DeviceSelectionPort for SecretsAdapters {
    type Device = <piv_io::RealSecretsBoundary as DeviceSelectionPort>::Device;

    fn discover_devices(&mut self) -> Result<Vec<DeviceCandidate>> {
        self.boundary.discover_devices()
    }

    fn open_device_by_serial(&mut self, serial: u32) -> Result<Self::Device> {
        self.boundary.open_device_by_serial(serial)
    }
}

impl DeviceSerialPort for SecretsAdapters {
    fn resolve_device_serial(&mut self, requested: Option<u32>) -> Result<u32> {
        self.boundary.resolve_device_serial(requested)
    }
}

impl SpareDeviceSerialPort for SecretsAdapters {
    fn resolve_spare_device_serial(&mut self, requested_spare_serial: Option<u32>) -> Result<u32> {
        self.boundary
            .resolve_spare_device_serial(requested_spare_serial)
    }
}

impl DevicePinPolicyPort for SecretsAdapters {
    fn device_requires_pin(&mut self, serial: u32) -> Result<bool> {
        self.boundary.device_requires_pin(serial)
    }
}

impl PinInputPort for SecretsAdapters {
    fn read_pin(&self) -> Result<SecretMaterial> {
        self.boundary.read_pin()
    }
}

impl SecretInputPort for SecretsAdapters {
    fn read_visible_secret(&self) -> Result<SecretMaterial> {
        self.boundary.read_visible_secret()
    }

    fn read_hidden_secret(&self, name: SecretName) -> Result<SecretMaterial> {
        self.boundary.read_hidden_secret(name)
    }

    fn read_stdin_secret(&self) -> Result<SecretMaterial> {
        self.boundary.read_stdin_secret()
    }
}

impl BootstrapSecretDocumentInputPort for SecretsAdapters {
    fn read_bootstrap_secret_document_noninteractive(&self) -> Result<BootstrapSecretDocument> {
        self.boundary
            .read_bootstrap_secret_document_noninteractive()
    }
}

impl SecretOutputPort for SecretsAdapters {
    fn write_secret(&self, secret: &SecretMaterial) -> Result<()> {
        self.boundary.write_secret(secret)
    }
}

impl ReportPort for SecretsAdapters {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()> {
        self.boundary.write_enroll_report(summary)
    }

    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()> {
        self.boundary.write_verify_report(summary)
    }
}
