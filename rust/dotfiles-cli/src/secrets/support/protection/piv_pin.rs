//! PIV PIN 検証を protection 境界内で完了する操作。

use crate::Result;

use super::ProtectedSecret;

/// PIV PIN を受け取る検証処理。
pub(crate) trait PivPinVerifier {
    fn verify(&mut self, pin: &[u8]) -> Result<()>;
}

/// PIV PIN の借用から検証処理までを `ProtectedSecret` の借用境界に閉じる。
pub(crate) fn verify_pin(pin: &ProtectedSecret, verifier: &mut impl PivPinVerifier) -> Result<()> {
    pin.with_secret(|bytes| verifier.verify(bytes))
}
