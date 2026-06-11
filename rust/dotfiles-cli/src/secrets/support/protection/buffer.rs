//! 入力 bytes の読み込み容量と zeroize 対象 allocation を同じ所有値で管理する buffer。

use std::io::{self, Write};

use anyhow::bail;
use zeroize::{Zeroize, Zeroizing};

use crate::Result;

use super::{ProtectedSecret, SecretSession};

/// 読み込み済み bytes と、その allocation に対応する任意の memory lock guard を所有する。
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
    /// allocation 全体を zeroize 管理へ入れ、memory lock は可能な場合だけ保持する。
    pub(crate) fn new(capacity: usize, session: &SecretSession) -> Result<Self> {
        let buffer = Zeroizing::new(vec![0; capacity]);
        let lock = session.lock_transient_buffer(buffer.as_ptr(), capacity);
        Ok(Self {
            buffer,
            len: 0,
            lock,
        })
    }

    /// 読み込み済み範囲を byte slice として返す。
    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.buffer[..self.len]
    }

    /// 端末 backspace 用に直前の byte を buffer から除く。
    pub(crate) fn pop_byte(&mut self) {
        if self.len > 0 {
            self.len -= 1;
            self.buffer[self.len].zeroize();
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

    /// 入力 allocation を、末尾改行除去後の raw bytes と任意の lock guard へ分解する。
    ///
    /// この関数は `Zeroizing<Vec<u8>>` の Drop 管理から一時的に raw `Vec<u8>` を取り出し、
    /// caller は返却された bytes と guard を分離して保持せず、直後に `ProtectedSecret` へ
    /// 移して zeroize ownership を再結合する責務を持つ。
    fn into_trimmed_bytes_and_lock(self) -> (Vec<u8>, Option<region::LockGuard>) {
        let mut this = self;
        let mut wrapped = std::mem::take(&mut this.buffer);
        let mut buffer = std::mem::take(&mut *wrapped);
        let len = this.len;
        let lock = this.lock.take();
        let len = if buffer[..len].ends_with(b"\r\n") {
            len - 2
        } else if buffer[..len].ends_with(b"\n") {
            len - 1
        } else {
            len
        };
        buffer.truncate(len);

        (buffer, lock)
    }

    /// 行入力 bytes を、保護済み値へ移す。
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
        let (buffer, lock) = self.into_trimmed_bytes_and_lock();
        Ok(session.protect_locked_secret_value(buffer, lock))
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
    use std::io::Write;

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
        let mut input = ProtectedInputBuffer::new(5, &session)?;
        input.write_all(b"abc\n")?;
        let secret = input.into_protected_secret_line(&session, 3, "too large")?;

        secret.with_secret(|secret| assert_secret_bytes_eq(secret, b"abc", "lf secret"));
        Ok(())
    }

    #[test]
    fn secret_line_accepts_exact_limit_with_crlf() -> Result<()> {
        let session = crate::secrets::support::protection::SecretSession::start()?;
        let mut input = ProtectedInputBuffer::new(5, &session)?;
        input.write_all(b"abc\r\n")?;
        let secret = input.into_protected_secret_line(&session, 3, "too large")?;

        secret.with_secret(|secret| assert_secret_bytes_eq(secret, b"abc", "crlf secret"));
        Ok(())
    }

    #[test]
    fn secret_line_rejects_body_past_limit_after_trim() -> Result<()> {
        let session = crate::secrets::support::protection::SecretSession::start()?;
        let mut input = ProtectedInputBuffer::new(5, &session)?;
        input.write_all(b"abcd\n")?;
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
