#[cfg(not(feature = "secrets-internal-test-stub"))]
pub(crate) mod bws_backend;
#[cfg(not(feature = "secrets-internal-test-stub"))]
pub(crate) mod bws_sdk;
#[cfg(feature = "secrets-internal-test-stub")]
pub(crate) mod internal_stub_bws;
#[cfg(not(feature = "secrets-internal-test-stub"))]
pub(crate) mod protection_bws;
