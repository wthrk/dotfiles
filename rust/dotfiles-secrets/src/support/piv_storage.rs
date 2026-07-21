//! PIV storage backend の技術的 byte/識別子変換。

#[cfg(not(feature = "secrets-internal-test-stub"))]
use sha2::{Digest, Sha256};

pub(crate) fn non_empty_payload(value: Option<Vec<u8>>) -> Option<Vec<u8>> {
    value.filter(|bytes| !bytes.is_empty())
}

#[cfg(not(feature = "secrets-internal-test-stub"))]
pub(crate) fn sha256_lowercase_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn resolve_exactly_one_serial(
    serials: &[u32],
    multiple_error: &str,
) -> crate::Result<u32> {
    match serials {
        [] => anyhow::bail!("no YubiKey detected"),
        [serial] => Ok(*serial),
        _ => anyhow::bail!("{multiple_error}"),
    }
}
