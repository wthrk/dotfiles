//! stdin からの secret 読み取り adapter。
//!
//! pipe または redirect された stdin から保護済み値を読み込む。

use anyhow::bail;
use zeroize::Zeroizing;

use crate::{
    secrets::support::protection::{ProtectedInputBuffer, SecretSession},
    Result,
};

use super::terminal;

/// stdin から 1 secret を読み、zeroize 保護済み bytes として返す。
///
/// stdin が TTY の場合は error で失敗する。
pub(super) fn read_stdin_bytes(limit: usize) -> Result<Zeroizing<Vec<u8>>> {
    if terminal::stdin_is_terminal() {
        bail!("--stdin requires pipe or redirect input");
    }
    let session = SecretSession::start()?;
    let input = ProtectedInputBuffer::read_line_from(std::io::stdin(), limit, &session)?;
    let protected =
        input.into_protected_secret_line(&session, limit, "stdin secret input is too large")?;
    Ok(Zeroizing::new(protected.with_secret(|b| b.to_vec())))
}
