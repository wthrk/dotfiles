//! 保護済み secret を一時借用で外部 consumer へ渡す protection primitive。

use anyhow::Result;

use super::ProtectedSecret;

/// `ProtectedSecret` の借用中だけ secret bytes を処理する consumer。
///
/// 実装側は受け取った bytes を保持・複製してはならず、呼び出し中の外部 API へ渡す用途に限定する。
pub(crate) trait SecretConsumer {
    fn consume(&mut self, bytes: &[u8]) -> Result<()>;
}

/// secret bytes の借用範囲を protection 内部に閉じ、consumer 実行後に参照を残さない。
pub(crate) fn consume(secret: &ProtectedSecret, consumer: &mut impl SecretConsumer) -> Result<()> {
    secret.with_secret(|bytes| consumer.consume(bytes))
}
