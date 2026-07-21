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
//! [`MgmKey::get_protected`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/mgm.rs)、
//! [`Error::NotFound`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/error.rs)
//! で直接確認する。
//!
//! `MgmKey::get_protected` の固定 source は、最初に management-slot metadata を query してから
//! protected data object を読む。両段階の `Error::NotFound` は同じ public `Result` で返され、
//! caller が origin を区別する API はない。よってこの module は `NotFound` を protected key
//! 不在へ再分類せず、全 error を source error のまま停止する。B0 default-key bootstrap は
//! 自動化しない。
//!
//! 同 source の `MgmKey::set_protected` は management key 更新後の protected/admin metadata
//! 操作の失敗を default 化または log-only にして `Ok(())` を返し得る。将来、人間が承認した
//! 移行 flow でこの API を使う場合も、その `Ok(())` を成功証拠にしてはならない。handle を
//! 捨てて開き直し、PIN verify → protected key read → authenticate → management metadata
//! `default == Some(false)` をすべて確認できるまで成功を返さない。本 repository にはその
//! 書換 flow を実装しない。

use crate::{Result, support::protection::ProtectedSecret};
use anyhow::Error;

pub(crate) fn verify_pin(yubikey: &mut yubikey::YubiKey, pin: &ProtectedSecret) -> Result<()> {
    pin.with_secret(|bytes| yubikey.verify_pin(bytes).map_err(Error::new))
}
pub(crate) fn authenticate_protected_management_key(
    yubikey: &mut yubikey::YubiKey,
    pin: &ProtectedSecret,
) -> Result<()> {
    verify_pin(yubikey, pin)?;
    let key = yubikey::MgmKey::get_protected(yubikey).map_err(Error::new)?;
    yubikey.authenticate(&key).map_err(Error::new)?;
    Ok(())
}
