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
//! [`YubiKey::change_pin`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/yubikey.rs)、
//! [`MgmKey::get_protected`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/mgm.rs)、
//! [`Error::NotFound`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/error.rs)
//! で直接確認する。
//!
//! PIN VERIFY の error mapping は upstream version 固定
//! [`Transaction::verify_pin`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/transaction.rs)
//! と [`Error::WrongPin`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/error.rs) を直接確認する。
//! [NIST SP 800-73pt2-5 §3.2.1](https://nvlpubs.nist.gov/nistpubs/SpecialPublications/NIST.SP.800-73pt2-5.pdf)
//! は raw status word `0x6983` を `authentication method blocked` と定義する。一方、固定 crate source
//! は card status を完全な利用者向け状態へ対応付ける API を提供しない。repository は raw status や
//! retry 回数を利用者向け状態へ再分類せず、SDK error を opaque のまま停止する。自動 retry、fallback、
//! PUK、reset は行わない。
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

#[cfg(any(test, not(feature = "secrets-internal-test-stub")))]
use crate::{Result, foundation::protection::ProtectedSecret};

#[cfg(not(feature = "secrets-internal-test-stub"))]
pub(crate) fn verify_pin(yubikey: &mut yubikey::YubiKey, pin: &ProtectedSecret) -> Result<()> {
    verify_pin_with(pin, |bytes| yubikey.verify_pin(bytes))
}

/// current/new PIN を SDK の `change_pin` へ保護 buffer の借用中だけ渡す。
///
/// この操作は `setup` または fresh enrollment が read-only preflight と confirmation を通過した後だけ
/// 呼ぶ。SDK error は retry count や reset/PUK state として再分類しない。
#[cfg(not(feature = "secrets-internal-test-stub"))]
pub(crate) fn change_pin(
    yubikey: &mut yubikey::YubiKey,
    current_pin: &ProtectedSecret,
    new_pin: &ProtectedSecret,
) -> Result<()> {
    current_pin.with_secret(|current| {
        new_pin.with_secret(|new| yubikey.change_pin(current, new).map_err(opaque_piv_error))
    })
}

/// PIV VERIFY callback へ `ProtectedSecret` の bytes をそのまま渡す共通境界。
///
/// PIN reader は CR/LF の行終端だけを除き、通常の PIN bytes を trim・文字列化・encoding 変換
/// しない。この関数も借用 bytes をそのまま VERIFY callback へ渡す。physical retry を増やさない
/// ため、caller はこの関数を 1 回だけ呼び、retry / fallback / PUK / reset を実装しない。
#[cfg(any(test, not(feature = "secrets-internal-test-stub")))]
pub(crate) fn verify_pin_with(
    pin: &ProtectedSecret,
    verify: impl FnOnce(&[u8]) -> std::result::Result<(), yubikey::Error>,
) -> Result<()> {
    pin.with_secret(|bytes| verify(bytes).map_err(verify_pin_failure))
}

/// fixed SDK の error を利用者向けに意味付けず、PIN/status/source text を伏せた opaque failure にする。
pub(crate) fn opaque_piv_error(error: yubikey::Error) -> anyhow::Error {
    anyhow::Error::new(error).context("YubiKey PIV operation failed")
}

/// VERIFY failure を opaque failure にする。
pub(crate) fn verify_pin_failure(error: yubikey::Error) -> anyhow::Error {
    opaque_piv_error(error)
}
#[cfg(test)]
mod tests {
    use super::verify_pin_failure;

    #[test]
    fn verify_failure_is_opaque() {
        let error = verify_pin_failure(yubikey::Error::WrongPin { tries: 3 });

        assert_eq!(error.to_string(), "YubiKey PIV operation failed");
    }
}
