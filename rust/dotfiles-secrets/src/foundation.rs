//! Feature-neutral technical primitives.

#[cfg(not(feature = "secrets-internal-test-stub"))]
pub(crate) mod aead;
pub(crate) mod protection;
