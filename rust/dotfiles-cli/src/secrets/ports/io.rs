//! process / terminal / stdio / report backend capability の port 契約。

use std::collections::BTreeMap;

use crate::Result;
use crate::secrets::domain::values::{EnrollSummary, VerifySummary};
use crate::secrets::support::protection::ProtectedSecret;

#[cfg_attr(test, mockall::automock)]
pub trait PinInputPort {
    fn read_pin(&self) -> Result<ProtectedSecret>;
}

#[cfg_attr(test, mockall::automock)]
pub trait SecretInputPort {
    fn read_bw_email_secret(&self) -> Result<ProtectedSecret>;
    fn read_bw_password_secret(&self) -> Result<ProtectedSecret>;
    fn read_bws_access_token_secret(&self) -> Result<ProtectedSecret>;
    fn read_streamed_secret(&self) -> Result<ProtectedSecret>;
}

#[cfg_attr(test, mockall::automock)]
pub trait RotationContinuationPort {
    fn continue_rotation(&self) -> Result<bool>;
}

#[cfg_attr(test, mockall::automock)]
pub trait BootstrapSecretDocumentInputPort {
    fn read_bootstrap_secret_fields(&self) -> Result<BTreeMap<String, ProtectedSecret>>;
}

#[cfg_attr(test, mockall::automock)]
pub trait SecretOutputPort {
    fn write_secret(&self, secret: &ProtectedSecret) -> Result<()>;
}

#[cfg_attr(test, mockall::automock)]
pub trait ReportPort {
    fn write_enroll_report(&self, summary: &EnrollSummary) -> Result<()>;
    fn write_verify_report(&self, summary: &VerifySummary) -> Result<()>;
}
