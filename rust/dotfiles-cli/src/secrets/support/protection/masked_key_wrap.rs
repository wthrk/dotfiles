//! 保護済み key material を固定 mask 付き wire bytes へ変換する test 用 primitive。

use anyhow::Result;

use super::ProtectedSecret;

/// 保護済み key material を prefix と XOR mask で包む。
///
/// この primitive は feature や外部製品の語彙を持たず、secret bytes の借用を
/// protection 内部に閉じたまま、呼び出し側が指定した wire prefix へ変換する。
pub(crate) fn wrap(key: &ProtectedSecret, prefix: &[u8], mask: u8) -> Vec<u8> {
    key.with_secret(|bytes| {
        let mut wrapped = Vec::with_capacity(prefix.len() + bytes.len());
        wrapped.extend_from_slice(prefix);
        wrapped.extend(bytes.iter().map(|byte| byte ^ mask));
        wrapped
    })
}

/// prefix と XOR mask で包まれた key material を保護済み secret へ戻す。
pub(crate) fn unwrap(wrapped_key: &[u8], prefix: &[u8], mask: u8) -> Result<ProtectedSecret> {
    let Some(masked) = wrapped_key.strip_prefix(prefix) else {
        anyhow::bail!("invalid masked content key");
    };
    let mut key = ProtectedSecret::new(masked.len())?;
    key.with_secret_mut(|bytes| {
        for (dst, source) in bytes.iter_mut().zip(masked.iter()) {
            *dst = *source ^ mask;
        }
    });
    Ok(key)
}
