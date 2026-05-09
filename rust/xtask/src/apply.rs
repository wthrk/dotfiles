use std::process::Command;

use crate::{Result, cli::ApplyTarget};
use anyhow::bail;

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
