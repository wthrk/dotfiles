use anyhow::Result;

use super::{ProtectedSecret, sealed_blob};

const STUB_WRAP_PREFIX: &[u8] = b"dotfiles-stub-wrapped-v1:";

pub(crate) fn wrap_content_key(key: &ProtectedSecret) -> Vec<u8> {
    key.with_secret(|bytes| {
        let mut wrapped = Vec::with_capacity(STUB_WRAP_PREFIX.len() + bytes.len());
        wrapped.extend_from_slice(STUB_WRAP_PREFIX);
        wrapped.extend(bytes.iter().map(|byte| byte ^ 0xa5));
        wrapped
    })
}

pub(crate) fn unwrap_content_key(wrapped_key: &[u8]) -> Result<ProtectedSecret> {
    let Some(masked) = wrapped_key.strip_prefix(STUB_WRAP_PREFIX) else {
        anyhow::bail!("invalid stub-wrapped content key");
    };
    let mut key = ProtectedSecret::new(masked.len())?;
    key.with_secret_mut(|bytes| {
        for (dst, source) in bytes.iter_mut().zip(masked.iter()) {
            *dst = *source ^ 0xa5;
        }
    });
    Ok(key)
}

pub(crate) fn zero_content_key() -> Result<ProtectedSecret> {
    ProtectedSecret::new(sealed_blob::CONTENT_KEY_LEN)
}
