//! secret 入力の読み込み容量と memory lock 範囲を同じ所有値で管理する buffer。

use std::io::{self, Read, Write};

use anyhow::bail;
use zeroize::Zeroizing;

use crate::Result;

use super::SecretSession;

/// 上限判定に必要な余剰 bytes まで lock 対象から外さない入力 buffer。
pub(crate) struct ProtectedInputBuffer {
    buffer: Zeroizing<Vec<u8>>,
    len: usize,
    _lock: Option<region::LockGuard>,
}

impl ProtectedInputBuffer {
    /// 読み込み先の allocation 全体を先に確保し、session がある場合は同じ範囲を lock する。
    pub(crate) fn new(capacity: usize, session: Option<&SecretSession>) -> Result<Self> {
        let buffer = Zeroizing::new(vec![0; capacity]);
        let lock = match session {
            Some(session) => Some(session.lock_transient_buffer(buffer.as_ptr(), capacity)?),
            None => None,
        };
        Ok(Self {
            buffer,
            len: 0,
            _lock: lock,
        })
    }

    /// raw 入力は改行を特別扱いせず、上限を超えた時点で拒否する。
    pub(crate) fn read_from(
        reader: impl Read,
        limit: usize,
        too_large_error: &'static str,
        session: Option<&SecretSession>,
    ) -> Result<Self> {
        let mut buffer = Self::new(limit + 1, session)?;
        let len = io::copy(&mut reader.take((limit + 1) as u64), &mut buffer)? as usize;
        if len > limit {
            bail!(too_large_error);
        }

        Ok(buffer)
    }

    /// 行入力の上限は、端末由来の末尾改行を取り除いた secret 本体に対して適用する。
    pub(crate) fn read_line_from(
        reader: impl Read,
        limit: usize,
        session: Option<&SecretSession>,
    ) -> Result<Self> {
        let read_limit = limit + 3;
        let mut buffer = Self::new(read_limit, session)?;
        buffer.len = io::copy(&mut reader.take(read_limit as u64), &mut buffer)? as usize;
        Ok(buffer)
    }

    /// 未初期化扱いの余剰容量を JSON parser へ見せない。
    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.buffer[..self.len]
    }

    /// 末尾 newline を除いた入力 bytes と読み込み時の lock guard を同時に所有境界へ移す。
    pub(crate) fn into_trimmed_bytes_and_lock(
        self,
    ) -> (Zeroizing<Vec<u8>>, Option<region::LockGuard>) {
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

    /// 行入力 secret の上限は、末尾改行を除いた保存対象 bytes に適用する。
    pub(crate) fn into_secret_line_and_lock(
        self,
        limit: usize,
        too_large_error: &'static str,
    ) -> Result<(Zeroizing<Vec<u8>>, Option<region::LockGuard>)> {
        let (buffer, lock) = self.into_trimmed_bytes_and_lock();
        if buffer.len() > limit {
            bail!(too_large_error);
        }
        Ok((buffer, lock))
    }
}

impl Write for ProtectedInputBuffer {
    /// `io::copy` からの書き込みは確保済み lock 範囲の内側に収める。
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = self.buffer.len().saturating_sub(self.len);
        let len = remaining.min(bytes.len());
        self.buffer[self.len..self.len + len].copy_from_slice(&bytes[..len]);
        self.len += len;
        Ok(len)
    }

    /// memory buffer writer なので flush は永続化や外部 I/O を伴わない。
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
        let input = ProtectedInputBuffer::read_line_from(Cursor::new(b"abc\n"), 3, None)?;
        let (secret, _lock) = input.into_secret_line_and_lock(3, "too large")?;

        assert_eq!(secret.as_slice(), b"abc");
        Ok(())
    }

    #[test]
    fn secret_line_accepts_exact_limit_with_crlf() -> Result<()> {
        let input = ProtectedInputBuffer::read_line_from(Cursor::new(b"abc\r\n"), 3, None)?;
        let (secret, _lock) = input.into_secret_line_and_lock(3, "too large")?;

        assert_eq!(secret.as_slice(), b"abc");
        Ok(())
    }

    #[test]
    fn secret_line_rejects_body_past_limit_after_trim() -> Result<()> {
        let input = ProtectedInputBuffer::read_line_from(Cursor::new(b"abcd\n"), 3, None)?;
        let err = input.into_secret_line_and_lock(3, "too large");

        assert!(err.is_err());
        Ok(())
    }
}
