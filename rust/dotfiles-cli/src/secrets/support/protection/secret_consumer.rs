//! 保護済み secret を一時借用で外部 consumer へ渡す protection primitive。

use std::io::Write;
use std::str;

use super::ProtectedSecret;
use crate::Result;

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

/// secret bytes を指定 writer へ一度だけ書き込み、借用範囲を protection 内部に閉じる。
pub(crate) fn write_to(secret: &ProtectedSecret, writer: &mut impl Write) -> Result<()> {
    secret.with_secret(|bytes| writer.write_all(bytes))?;
    Ok(())
}

/// secret bytes を UTF-8 文字列として一時借用し、closure 実行中だけ利用する。
pub(crate) fn with_utf8_secret<R>(
    secret: &ProtectedSecret,
    borrow: impl FnOnce(&str) -> Result<R>,
) -> Result<R> {
    secret.with_secret(|bytes| {
        let text = str::from_utf8(bytes)
            .map_err(|_| anyhow::anyhow!("bws access token is not valid UTF-8"))?;
        borrow(text)
    })
}
