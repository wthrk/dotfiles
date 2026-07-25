//! Root composition boundary.
//!
//! Root wiring is intentionally isolated from feature layers.  It is the only
//! route from the public crate entry to the feature entrypoint.

pub(crate) mod bootstrap;
