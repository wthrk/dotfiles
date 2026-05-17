//! 入力 bytes の読み込み容量と memory lock 範囲を同じ所有値で管理する buffer。

use std::io::{self, Read, Write};

use anyhow::bail;

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
        mut reader: impl Read,
        limit: usize,
        too_large_error: &'static str,
        session: &SecretSession,
    ) -> Result<Self> {
        let mut buffer = Self::new(limit + 1, session)?;
        buffer.read_capped_from(&mut reader, limit + 1)?;
        if buffer.len > limit {
            bail!(too_large_error);
        }

        Ok(buffer)
    }

    /// reader から行入力用の bytes を読み込む。
    ///
    /// 末尾改行を除いた後に上限判定できるよう、CRLF 分の余剰容量を確保する。
    pub(crate) fn read_line_from(
        mut reader: impl Read,
        limit: usize,
        session: &SecretSession,
    ) -> Result<Self> {
        let read_limit = limit + 3;
        let mut buffer = Self::new(read_limit, session)?;
        buffer.read_capped_from(&mut reader, read_limit)?;
        Ok(buffer)
    }

    fn read_capped_from(&mut self, reader: &mut impl Read, cap: usize) -> io::Result<()> {
        let target_len = cap.min(self.buffer.len());
        while self.len < target_len {
            let read = reader.read(&mut self.buffer[self.len..target_len])?;
            if read == 0 {
                break;
            }
            self.len += read;
        }
        Ok(())
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
        if self.len > 0 {
            self.len -= 1;
            self.buffer[self.len] = 0;
        }
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
        buffer[len..].fill(0);
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
        buffer[len..].fill(0);
        buffer.truncate(len);
        session.protect_locked_secret_value(buffer, Some(_lock))
    }
}

impl Write for ProtectedInputBuffer {
    /// bytes を確保済み allocation の残り容量へ書き込む。
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = self.buffer.len().saturating_sub(self.len);
        if bytes.len() > remaining {
            self.buffer[..self.len].fill(0);
            self.len = 0;
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "protected input buffer capacity exceeded",
            ));
        }
        self.buffer[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
        Ok(bytes.len())
    }

    /// memory buffer writer として flush を完了扱いにする。
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use crate::Result;

    use super::ProtectedInputBuffer;

    #[test]
    fn secret_line_accepts_exact_limit_with_lf() -> Result<()> {
        let session = crate::secrets::support::protection::SecretSession::start()?;
        let input = ProtectedInputBuffer::read_line_from(Cursor::new(b"abc\n"), 3, &session)?;
        let secret = input.into_protected_secret_line(&session, 3, "too large")?;

        secret.with_secret(|secret| assert_eq!(secret, b"abc"));
        Ok(())
    }

    #[test]
    fn secret_line_accepts_exact_limit_with_crlf() -> Result<()> {
        let session = crate::secrets::support::protection::SecretSession::start()?;
        let input = ProtectedInputBuffer::read_line_from(Cursor::new(b"abc\r\n"), 3, &session)?;
        let secret = input.into_protected_secret_line(&session, 3, "too large")?;

        secret.with_secret(|secret| assert_eq!(secret, b"abc"));
        Ok(())
    }

    #[test]
    fn secret_line_rejects_body_past_limit_after_trim() -> Result<()> {
        let session = crate::secrets::support::protection::SecretSession::start()?;
        let input = ProtectedInputBuffer::read_line_from(Cursor::new(b"abcd\n"), 3, &session)?;
        let err = input.into_protected_secret_line(&session, 3, "too large");

        assert!(err.is_err());
        Ok(())
    }

    #[test]
    fn write_failure_zeroizes_existing_bytes() -> Result<()> {
        let session = crate::secrets::support::protection::SecretSession::start()?;
        let mut input = ProtectedInputBuffer::new(3, &session)?;
        input.write_all(b"abc")?;
        let err = input.write_all(b"d");

        assert!(err.is_err());
        assert_eq!(input.as_slice(), b"");
        Ok(())
    }
}
