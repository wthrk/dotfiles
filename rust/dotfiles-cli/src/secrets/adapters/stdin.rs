//! stdin からの secret 読み取り adapter。
//!
//! pipe または redirect された stdin から保護済み値を読み込む。

use crate::{
    secrets::support::protection::{ProtectedInputBuffer, ProtectedSecret, SecretSession},
    Result,
};
use anyhow::bail;

use super::terminal;

/// stdin から 1 secret を読み、現在の session の保護済み値として返す。
///
/// 読み込み時の lock guard を引き継ぎ、unlock は値の破棄後に遅延させる。
pub(crate) fn read_protected_stdin_secret(
    limit: usize,
    session: &SecretSession,
) -> Result<ProtectedSecret<'_>> {
    if terminal::stdin_is_terminal() {
        bail!("--stdin requires pipe or redirect input");
    }
    let input = ProtectedInputBuffer::read_line_from(std::io::stdin(), limit, session)?;
    input.into_protected_secret_line(session, limit, "stdin secret input is too large")
}
