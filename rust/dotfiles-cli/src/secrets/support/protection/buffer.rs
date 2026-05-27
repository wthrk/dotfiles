//! 入力 bytes の読み込み容量と memory lock 範囲を同じ所有値で管理する buffer。

use std::io::{self, Read, Write};

use anyhow::{anyhow, bail};
use zeroize::{Zeroize, Zeroizing};

use crate::Result;

use super::{ProtectedSecret, SecretSession};

/// 読み込み済み bytes と、その allocation に対応する memory lock guard を所有する。
///
/// 上限超過判定に使う余剰 bytes も同じ allocation に含める。
pub(crate) struct ProtectedInputBuffer {
    buffer: Zeroizing<Vec<u8>>,
    len: usize,
    lock: Option<region::LockGuard>,
}

impl ProtectedInputBuffer {
    /// 指定容量の読み込み先 allocation を作る。
    ///
    /// allocation 全体を現在の session の memory lock 範囲へ入れる。
    pub(crate) fn new(capacity: usize, session: &SecretSession) -> Result<Self> {
        let buffer = Zeroizing::new(vec![0; capacity]);
        let lock = session.lock_transient_buffer(buffer.as_ptr(), capacity)?;
        Ok(Self {
            buffer,
            len: 0,
            lock: Some(lock),
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
        buffer.read_line_capped_from(&mut reader, read_limit)?;
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

    /// reader から 1 行ぶんだけ読み込み、最初の LF か `cap` 到達で停止する。
    ///
    /// この関数は停止地点より後ろの bytes を読み捨てず reader 側へ残す。caller は trailing bytes が
    /// 未読のまま残り得る前提で、その後の再読込や追加 prompt の境界を管理する責務を持つ。
    fn read_line_capped_from(&mut self, reader: &mut impl Read, cap: usize) -> io::Result<()> {
        let target_len = cap.min(self.buffer.len());
        let mut byte = [0u8; 1];
        while self.len < target_len {
            let read = reader.read(&mut byte)?;
            if read == 0 {
                break;
            }
            self.buffer[self.len] = byte[0];
            self.len += 1;
            if matches!(byte[0], b'\n') {
                break;
            }
        }
        Ok(())
    }

    /// 読み込み済み範囲を byte slice として返す。
    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.buffer[..self.len]
    }

    /// 端末 backspace 用に直前の byte を buffer から除く。
    pub(crate) fn pop_byte(&mut self) {
        if self.len > 0 {
            self.len -= 1;
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

    /// lock 済み入力 allocation を、末尾改行除去後の raw bytes と lock guard へ分解する。
    ///
    /// この関数は `Zeroizing<Vec<u8>>` の Drop 管理から一時的に raw `Vec<u8>` を取り出し、
    /// 同じ allocation を保護している `LockGuard` と対で返す safety boundary である。
    /// caller は返却された bytes と guard を分離して保持せず、直後に `ProtectedSecret` へ
    /// 移して zeroize/lock ownership を再結合する責務を持つ。
    ///
    /// lock guard が欠落している場合は保護境界不成立として `Err` を返し、lock なしの raw
    /// bytes だけを返さない。末尾改行の trim は ownership 移譲前に同一 allocation 上で完了する。
    fn into_trimmed_bytes_and_lock(self) -> Result<(Vec<u8>, region::LockGuard)> {
        let mut this = self;
        let mut wrapped = std::mem::take(&mut this.buffer);
        let mut buffer = std::mem::take(&mut *wrapped);
        let len = this.len;
        let lock = this
            .lock
            .take()
            .ok_or_else(|| anyhow!("protected input buffer lock missing"))?;
        let len = if buffer[..len].ends_with(b"\r\n") {
            len - 2
        } else if buffer[..len].ends_with(b"\n") {
            len - 1
        } else {
            len
        };
        buffer.truncate(len);

        Ok((buffer, lock))
    }

    /// 行入力 bytes を、同じ memory lock guard を引き継ぐ保護済み値へ移す。
    ///
    /// 上限は末尾改行を除いた bytes に適用し、超過時は指定 error で失敗する。
    pub(crate) fn into_protected_secret_line(
        self,
        session: &SecretSession,
        limit: usize,
        too_large_error: &'static str,
    ) -> Result<ProtectedSecret> {
        if self.trimmed_len() > limit {
            bail!(too_large_error);
        }
        let (buffer, lock) = self.into_trimmed_bytes_and_lock()?;
        session.protect_locked_secret_value(buffer, lock)
    }
}

impl Write for ProtectedInputBuffer {
    /// bytes を確保済み allocation の残り容量へ書き込む。
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = self.buffer.len().saturating_sub(self.len);
        if bytes.len() > remaining {
            self.buffer[..self.len].zeroize();
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
    use sha2::{Digest, Sha256};

    use super::ProtectedInputBuffer;

    fn assert_secret_bytes_eq(actual: &[u8], expected: &[u8], label: &str) {
        let actual_digest: [u8; 32] = Sha256::digest(actual).into();
        let expected_digest: [u8; 32] = Sha256::digest(expected).into();

        assert_eq!(actual.len(), expected.len(), "{label} length mismatch");
        assert_eq!(actual_digest, expected_digest, "{label} digest mismatch");
    }

    #[test]
    fn secret_line_accepts_exact_limit_with_lf() -> Result<()> {
        let session = crate::secrets::support::protection::SecretSession::start()?;
        let input = ProtectedInputBuffer::read_line_from(Cursor::new(b"abc\n"), 3, &session)?;
        let secret = input.into_protected_secret_line(&session, 3, "too large")?;

        secret.with_secret(|secret| assert_secret_bytes_eq(secret, b"abc", "lf secret"));
        Ok(())
    }

    #[test]
    fn secret_line_accepts_exact_limit_with_crlf() -> Result<()> {
        let session = crate::secrets::support::protection::SecretSession::start()?;
        let input = ProtectedInputBuffer::read_line_from(Cursor::new(b"abc\r\n"), 3, &session)?;
        let secret = input.into_protected_secret_line(&session, 3, "too large")?;

        secret.with_secret(|secret| assert_secret_bytes_eq(secret, b"abc", "crlf secret"));
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
    fn read_line_from_stops_at_first_newline() -> Result<()> {
        let session = crate::secrets::support::protection::SecretSession::start()?;
        let mut cursor = Cursor::new(b"first\nsecond\n");
        let first = ProtectedInputBuffer::read_line_from(&mut cursor, 16, &session)?;
        let second = ProtectedInputBuffer::read_line_from(&mut cursor, 16, &session)?;

        assert_eq!(first.as_slice(), b"first\n");
        assert_eq!(second.as_slice(), b"second\n");
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
