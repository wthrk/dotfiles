//! internal test stub feature 用の selected-device 実装。

pub(super) const ROUTE_LABEL: &str = "stub";

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/secrets_internal_stub/piv_io_internal_stub.rs"
));
