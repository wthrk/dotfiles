//! ローカル環境へ変更を適用する開発者向けコマンド。
//!
//! 適用前に静的検証を通し、その後 `dotfiles-cli switch` を呼ぶ。xtask 自体には検証や適用の
//! 中身を持たせず、実際の挙動は checks crate と公開 CLI に寄せる。

use std::process::Command;

use crate::{Result, cli::ApplyTarget};
use anyhow::bail;

/// 静的検証が成功した場合だけ、指定対象の switch を起動する。
pub fn run(target: ApplyTarget) -> Result<()> {
    run_checks()?;
    let mut command = Command::new("cargo");
    command.args(["run", "--package", "dotfiles-cli", "--", "switch"]);
    match target {
        ApplyTarget::All => {
            command.arg("all");
        }
        ApplyTarget::HomeManager => {
            command.arg("home");
        }
    }
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        bail!("apply failed: {status}")
    }
}

/// 適用前検証は runtime VM と実行テストを起動しない `dotfiles-checks static` に限定する。
/// workspace test や CLI/shell fixture は明示的な `cargo xtask check test` の責務であり、適用経路へ混ぜない。
fn run_checks() -> Result<()> {
    let status = Command::new("cargo")
        .args(["run", "--package", "dotfiles-checks", "--", "static"])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        bail!("checks failed: {status}")
    }
}
