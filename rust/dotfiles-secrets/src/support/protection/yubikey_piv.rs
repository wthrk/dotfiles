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
//! PIN VERIFY の error mapping は upstream version 固定
//! [`Transaction::verify_pin`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/transaction.rs)
//! と [`Error::WrongPin`](https://docs.rs/crate/yubikey/0.9.0-pre.0/source/src/error.rs) を直接確認する。
//! [NIST SP 800-73pt2-5 §3.2.1](https://nvlpubs.nist.gov/nistpubs/SpecialPublications/NIST.SP.800-73pt2-5.pdf)
//! は raw status word `0x6983` を `authentication method blocked` と定義する。一方、固定 crate source
//! はその `0x6983` (`AuthBlockedError`) と `0x63Cx` (`VerifyFailError`) の両方を `WrongPin { tries }` に
//! 写像するため、`tries == 0` から生の status word は復元できない。
//! [Yubico PIV VERIFY specification](https://docs.yubico.com/yesdk/users-manual/application-piv/apdu/verify.html#verify-pin)
//! が定義する応答は成功の `0x9000` と PIN 不一致の `0x63CX` である。`0x6983` の raw 意味は前記 NIST、
//! `0x6983` と `0x63C0` を同じ crate 値へ写像する根拠は上記の固定 `Transaction::verify_pin` source
//! と区別する。従って
//! `tries == 0` は raw status を失った opaque failure として停止する。PIN の正誤、残試行数、
//! blocked 状態のいずれもこの SDK 値からは表示・判定しない。`tries > 0` だけを PIN rejected として
//! 残試行回数付きで表示する。自動 retry、fallback、PUK、reset は行わない。
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
use crate::{Result, support::protection::ProtectedSecret};
use anyhow::Error;

#[cfg(not(feature = "secrets-internal-test-stub"))]
pub(crate) fn verify_pin(yubikey: &mut yubikey::YubiKey, pin: &ProtectedSecret) -> Result<()> {
    verify_pin_with(pin, |bytes| yubikey.verify_pin(bytes))
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

/// `yubikey::Error` が `anyhow` 化される前に、VERIFY の型付き結果を利用者向け失敗へ写像する。
///
/// `tries == 0` は crate の lossy mapping により `0x6983` と `0x63C0` を区別できない。そのため
/// PIN が誤り、blocked、残試行ゼロのいずれとも断定せず opaque failure として伝播する。PIN 値は
/// この error に含めない。
pub(crate) fn verify_pin_failure(error: yubikey::Error) -> Error {
    match error {
        yubikey::Error::WrongPin { tries: 0 } => Error::new(RawPivStatusUnavailable),
        yubikey::Error::WrongPin { tries } => {
            Error::msg(format!("PIV PIN rejected; retries remaining: {tries}"))
        }
        error => Error::new(error),
    }
}

#[derive(Debug)]
struct RawPivStatusUnavailable;

impl std::fmt::Display for RawPivStatusUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "PIV VERIFY failed; this YubiKey SDK result does not preserve the raw card status, so PIN correctness and device state are undetermined",
        )
    }
}

impl std::error::Error for RawPivStatusUnavailable {}
#[cfg(test)]
mod tests {
    use super::verify_pin_failure;

    #[test]
    fn verify_rejection_reports_remaining_retries() {
        let error = verify_pin_failure(yubikey::Error::WrongPin { tries: 3 });

        assert_eq!(error.to_string(), "PIV PIN rejected; retries remaining: 3");
    }

    #[test]
    fn zero_retries_is_an_opaque_failure() {
        let error = verify_pin_failure(yubikey::Error::WrongPin { tries: 0 });

        assert_eq!(
            error.to_string(),
            "PIV VERIFY failed; this YubiKey SDK result does not preserve the raw card status, so PIN correctness and device state are undetermined"
        );
    }
}
