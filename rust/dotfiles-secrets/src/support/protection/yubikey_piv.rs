//! PIV PIN / management-key operation を protection 境界内で完結する。
//!
//! ## 出典と適用判断
//!
//! repository 正本は
//! [`secret-recovery-spec.md` の「YubiKey PIV PIN の利用境界」](../../../../../docs/secret-recovery/secret-recovery-spec.md#yubikey-piv-pin-の利用境界) と
//! [`yubikey-secret-storage-design.md` の「Slot」](../../../../../docs/secret-recovery/yubikey-secret-storage-design.md#slot)
//! である。管理操作だけが hidden TTY から得た PIN を使い、復旧・read・unwrap path はこの
//! module を呼ばず PIN を要求しない。
//!
//! vendor の PIV 管理操作と management key の位置付けは
//! [YubiKey Manager PIV Commands](https://docs.yubico.com/software/yubikey/tools/ykman/PIV_Commands.html)
//! を、固定 SDK API は `yubikey` 0.9.0-pre.0 の
//! [`YubiKey::verify_pin` / `YubiKey::authenticate`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/yubikey.rs)、
//! [`MgmKey::get_default` / `get_protected` / `generate_for` / `set_protected`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/mgm.rs)、
//! [`Error::NotFound`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/error.rs)
//! で直接確認する。
//!
//! `verify_pin`、`authenticate`、`get_default`、`generate_for`、`set_protected` の error は意味を
//! 再分類せず source error のまま停止する。`get_protected` の戻り値が**正確に** `Error::NotFound`
//! の時だけ `Missing` を返し、caller が management-slot metadata の `default == Some(true)` を
//! 確認した B0 bootstrap candidate に限って `get_default` → `authenticate` → `generate_for` →
//! `set_protected` へ進める。その他の error、metadata の未知値、認証失敗を default-key fallback、
//! retry、成功へ写像しない。

use crate::{Result, support::protection::ProtectedSecret};
use anyhow::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProtectedManagementKeyState {
    Authenticated,
    Missing,
}
pub(crate) fn verify_pin(yubikey: &mut yubikey::YubiKey, pin: &ProtectedSecret) -> Result<()> {
    pin.with_secret(|bytes| yubikey.verify_pin(bytes).map_err(Error::new))
}
pub(crate) fn authenticate_protected_management_key(
    yubikey: &mut yubikey::YubiKey,
    pin: &ProtectedSecret,
) -> Result<ProtectedManagementKeyState> {
    verify_pin(yubikey, pin)?;
    match yubikey::MgmKey::get_protected(yubikey) {
        Ok(key) => {
            yubikey.authenticate(&key).map_err(Error::new)?;
            Ok(ProtectedManagementKeyState::Authenticated)
        }
        Err(yubikey::Error::NotFound) => Ok(ProtectedManagementKeyState::Missing),
        Err(error) => Err(Error::new(error)),
    }
}
pub(crate) fn bootstrap_pin_protected_management_key(yubikey: &mut yubikey::YubiKey) -> Result<()> {
    let default = yubikey::MgmKey::get_default(yubikey).map_err(Error::new)?;
    yubikey.authenticate(&default).map_err(Error::new)?;
    let protected = yubikey::MgmKey::generate_for(yubikey, &mut yubikey_rand::rngs::SysRng)
        .map_err(Error::new)?;
    protected.set_protected(yubikey).map_err(Error::new)?;
    Ok(())
}
