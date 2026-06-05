//! PIV PIN 検証を protection 境界内で完了する操作。

use crate::{Result, support::protection::ProtectedSecret};

/// PIV PIN を借用中の bytes として受け取る検証処理。
///
/// implementor は受け取った PIN bytes を呼び出し中だけ利用し、保存、ログ出力、エラー文脈化を
/// してはならない。caller は `verify_pin` を通して `ProtectedSecret` の借用境界内だけで呼ぶ。
pub(crate) trait PivPinVerifier {
    fn verify(&mut self, pin: &[u8]) -> Result<()>;
}

/// PIV PIN の借用から検証処理までを `ProtectedSecret` の借用境界に閉じる。
pub(crate) fn verify_pin(pin: &ProtectedSecret, verifier: &mut impl PivPinVerifier) -> Result<()> {
    pin.with_secret(|bytes| verifier.verify(bytes))
}
