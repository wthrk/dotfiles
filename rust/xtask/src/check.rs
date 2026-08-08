//! `cargo xtask check` から実際の検証クレートを起動する層。
//!
//! 利用者が叩くコマンドは xtask に固定し、検証内容は `dotfiles-checks` へ集約する。
//! これにより xtask が独自にテスト手順を再実装しない。

use std::process::Command;

use anyhow::bail;

use crate::{
    Result,
    cli::{CheckTarget, RuntimeScenario},
};

/// xtask のサブコマンドを `dotfiles-checks` のサブコマンドへ 1 対 1 で変換する。
pub fn run(target: Option<CheckTarget>) -> Result<()> {
    let mut command = Command::new("cargo");
    command.args(["run", "--package", "dotfiles-checks", "--"]);
    match target {
        None => {}
        Some(CheckTarget::Static) => {
            command.arg("static");
        }
        Some(CheckTarget::Runtime {
            scenario,
            source_hash,
        }) => match scenario {
            Some(RuntimeScenario::Full) | None => {
                command.arg("integration");
                if let Some(source_hash) = source_hash {
                    command.arg("--source-hash").arg(source_hash);
                }
            }
        },
        Some(CheckTarget::All) => {
            command.arg("all");
        }
    }
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        bail!("checks failed: {status}")
    }
}
