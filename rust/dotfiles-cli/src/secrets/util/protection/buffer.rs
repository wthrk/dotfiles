//! 入力 bytes の読み込み容量と memory lock 範囲を同じ所有値で管理する buffer。

use std::{
    fmt,
    io::{self, Read, Write},
};

use anyhow::bail;
use serde::de::{self, DeserializeSeed, Visitor};

use crate::Result;

use super::{ProtectedSecret, SecretBytes, SecretSession};

/// 読み込み済み bytes と、その allocation に対応する memory lock guard を所有する。
///
/// 上限超過判定に使う余剰 bytes も同じ allocation に含める。
pub(crate) struct ProtectedInputBuffer {
    buffer: SecretBytes,
    len: usize,
    _lock: region::LockGuard,
}

impl ProtectedInputBuffer {
    /// 指定容量の読み込み先 allocation を作る。
    ///
    /// allocation 全体を現在の session の memory lock 範囲へ入れる。
    pub(crate) fn new(capacity: usize, session: &SecretSession) -> Result<Self> {
        let buffer = SecretBytes::new(vec![0; capacity]);
        let lock = session.lock_transient_buffer(buffer.as_ptr(), capacity)?;
        Ok(Self {
            buffer,
            len: 0,
            _lock: lock,
        })
    }

    /// reader から最大 `limit + 1` bytes を読み込む。
    ///
    /// `limit` を超えた場合は指定 error で失敗する。
    pub(crate) fn read_from(
        reader: impl Read,
        limit: usize,
        too_large_error: &'static str,
        session: &SecretSession,
    ) -> Result<Self> {
        let mut buffer = Self::new(limit + 1, session)?;
        let len = io::copy(&mut reader.take((limit + 1) as u64), &mut buffer)? as usize;
        if len > limit {
            bail!(too_large_error);
        }

        Ok(buffer)
    }

    /// reader から行入力用の bytes を読み込む。
    ///
    /// 末尾改行を除いた後に上限判定できるよう、CRLF 分の余剰容量を確保する。
    pub(crate) fn read_line_from(
        reader: impl Read,
        limit: usize,
        session: &SecretSession,
    ) -> Result<Self> {
        let read_limit = limit + 3;
        let mut buffer = Self::new(read_limit, session)?;
        buffer.len = io::copy(&mut reader.take(read_limit as u64), &mut buffer)? as usize;
        Ok(buffer)
    }

    /// reader から newline までの行入力 bytes を読み込む。
    ///
    /// TTY prompt では EOF を待たず、LF を読んだ時点で入力完了にする。
    pub(crate) fn read_line_until_newline_from(
        mut reader: impl Read,
        limit: usize,
        session: &SecretSession,
    ) -> Result<Self> {
        let read_limit = limit + 3;
        let mut buffer = Self::new(read_limit, session)?;
        let mut byte = [0u8; 1];
        while buffer.len < read_limit {
            if reader.read(&mut byte)? == 0 {
                break;
            }
            buffer.write_all(&byte)?;
            if byte[0] == b'\n' {
                break;
            }
        }
        Ok(buffer)
    }

    /// 読み込み済み範囲を byte slice として返す。
    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.buffer[..self.len]
    }

    /// 読み込み済み範囲を in-place 暗号処理の書き込み先として返す。
    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.buffer[..self.len]
    }

    /// 端末 backspace 用に直前の byte を buffer から除く。
    pub(crate) fn pop_byte(&mut self) {
        self.len = self.len.saturating_sub(1);
    }

    fn trimmed_len(&self) -> usize {
        if self.as_slice().ends_with(b"\r\n") {
            self.len - 2
        } else if self.as_slice().ends_with(b"\n") {
            self.len - 1
        } else {
            self.len
        }
    }

    fn into_trimmed_bytes_and_lock(self) -> (SecretBytes, region::LockGuard) {
        let Self { buffer, len, _lock } = self;
        let mut buffer = buffer;
        let len = if buffer[..len].ends_with(b"\r\n") {
            len - 2
        } else if buffer[..len].ends_with(b"\n") {
            len - 1
        } else {
            len
        };
        buffer.truncate(len);

        (buffer, _lock)
    }

    /// 行入力 bytes を、同じ memory lock guard を引き継ぐ保護済み値へ移す。
    ///
    /// 上限は末尾改行を除いた bytes に適用し、超過時は指定 error で失敗する。
    pub(crate) fn into_protected_secret_line<'session>(
        self,
        session: &'session SecretSession,
        limit: usize,
        too_large_error: &'static str,
    ) -> Result<ProtectedSecret<'session>> {
        if self.trimmed_len() > limit {
            bail!(too_large_error);
        }
        let (buffer, lock) = self.into_trimmed_bytes_and_lock();
        session.protect_locked_secret_value(buffer, Some(lock))
    }

    /// 読み込み済み bytes を、改行除去せず保護済み値へ移す。
    ///
    /// JSON string や復号結果など、入力形式側で bytes が確定している値に使う。
    pub(crate) fn into_protected_secret<'session>(
        self,
        session: &'session SecretSession,
    ) -> Result<ProtectedSecret<'session>> {
        let Self { buffer, len, _lock } = self;
        let mut buffer = buffer;
        buffer.truncate(len);
        session.protect_locked_secret_value(buffer, Some(_lock))
    }

    /// serde の string value を現在の session に属する buffer へ直接 decode する seed を返す。
    pub(crate) fn serde_string_seed(
        limit: usize,
        session: &SecretSession,
    ) -> ProtectedInputBufferStringSeed<'_> {
        ProtectedInputBufferStringSeed { limit, session }
    }
}

/// serde string value を `ProtectedInputBuffer` として受け取る decode seed。
pub(crate) struct ProtectedInputBufferStringSeed<'session> {
    limit: usize,
    session: &'session SecretSession,
}

impl<'de, 'session> DeserializeSeed<'de> for ProtectedInputBufferStringSeed<'session> {
    type Value = ProtectedInputBuffer;

    fn deserialize<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(ProtectedInputBufferStringVisitor {
            limit: self.limit,
            session: self.session,
        })
    }
}

struct ProtectedInputBufferStringVisitor<'session> {
    limit: usize,
    session: &'session SecretSession,
}

impl<'de, 'session> Visitor<'de> for ProtectedInputBufferStringVisitor<'session> {
    type Value = ProtectedInputBuffer;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("protected input string")
    }

    fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
    where
        E: de::Error,
    {
        if value.len() > self.limit {
            return Err(de::Error::custom("protected input is too large"));
        }
        let mut input =
            ProtectedInputBuffer::new(value.len(), self.session).map_err(de::Error::custom)?;
        input
            .write_all(value.as_bytes())
            .map_err(de::Error::custom)?;
        Ok(input)
    }
}

impl Write for ProtectedInputBuffer {
    /// bytes を確保済み allocation の残り容量へ書き込む。
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = self.buffer.len().saturating_sub(self.len);
        let len = remaining.min(bytes.len());
        self.buffer[self.len..self.len + len].copy_from_slice(&bytes[..len]);
        self.len += len;
        Ok(len)
    }

    /// memory buffer writer として flush を完了扱いにする。
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::Result;

    use super::ProtectedInputBuffer;

    #[test]
    fn secret_line_accepts_exact_limit_with_lf() -> Result<()> {
        let session = crate::secrets::util::protection::SecretSession::start()?;
        let input = ProtectedInputBuffer::read_line_from(Cursor::new(b"abc\n"), 3, &session)?;
        let secret = input.into_protected_secret_line(&session, 3, "too large")?;

        secret.with_secret(|secret| assert_eq!(secret, b"abc"));
        Ok(())
    }

    #[test]
    fn secret_line_accepts_exact_limit_with_crlf() -> Result<()> {
        let session = crate::secrets::util::protection::SecretSession::start()?;
        let input = ProtectedInputBuffer::read_line_from(Cursor::new(b"abc\r\n"), 3, &session)?;
        let secret = input.into_protected_secret_line(&session, 3, "too large")?;

        secret.with_secret(|secret| assert_eq!(secret, b"abc"));
        Ok(())
    }

    #[test]
    fn secret_line_rejects_body_past_limit_after_trim() -> Result<()> {
        let session = crate::secrets::util::protection::SecretSession::start()?;
        let input = ProtectedInputBuffer::read_line_from(Cursor::new(b"abcd\n"), 3, &session)?;
        let err = input.into_protected_secret_line(&session, 3, "too large");

        assert!(err.is_err());
        Ok(())
    }
}
