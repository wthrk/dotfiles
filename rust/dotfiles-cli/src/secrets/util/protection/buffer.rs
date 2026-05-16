//! secret 入力を読み込む前に容量と memory lock 範囲を確定する buffer。

use std::io::{self, Read, Write};

use anyhow::bail;
use zeroize::Zeroizing;

use crate::Result;

use super::SecretMemoryGuard;

/// 上限超過検出用の余剰 1 byte まで同じ memory lock 範囲で保持する入力 buffer。
pub(crate) struct ProtectedInputBuffer {
    buffer: Zeroizing<Vec<u8>>,
    len: usize,
    _lock: Option<region::LockGuard>,
}

impl ProtectedInputBuffer {
    /// caller が指定した容量全体を確保し、必要なら読み込み前に lock する。
    pub(crate) fn new(capacity: usize, memory: Option<&SecretMemoryGuard>) -> Result<Self> {
        let buffer = Zeroizing::new(vec![0; capacity]);
        let lock = match memory {
            Some(memory) => Some(memory.lock_transient_buffer(buffer.as_ptr(), capacity)?),
            None => None,
        };
        Ok(Self {
            buffer,
            len: 0,
            _lock: lock,
        })
    }

    /// `limit + 1` byte だけ読み、超過判定に使った byte も lock 範囲外へ出さない。
    pub(crate) fn read_from(
        reader: impl Read,
        limit: usize,
        too_large_error: &'static str,
        memory: Option<&SecretMemoryGuard>,
    ) -> Result<Self> {
        let mut buffer = Self::new(limit + 1, memory)?;
        let len = io::copy(&mut reader.take((limit + 1) as u64), &mut buffer)? as usize;
        if len > limit {
            bail!(too_large_error);
        }

        Ok(buffer)
    }

    /// JSON parse には読み込み済みの範囲だけを渡す。
    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.buffer[..self.len]
    }

    /// 末尾 newline を除いた入力 bytes と読み込み時の lock guard を同じ所有境界へ渡す。
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
}

impl Write for ProtectedInputBuffer {
    /// `io::copy` の書き込み先として、確保済みの lock 範囲を超えた bytes は受け取らない。
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = self.buffer.len().saturating_sub(self.len);
        let len = remaining.min(bytes.len());
        self.buffer[self.len..self.len + len].copy_from_slice(&bytes[..len]);
        self.len += len;
        Ok(len)
    }

    /// 入力 buffer は外部 writer を持たないため flush で追加処理を行わない。
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
