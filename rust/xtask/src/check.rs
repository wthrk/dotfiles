use std::process::Command;

use anyhow::bail;

use crate::{
    Result,
    cli::{CheckTarget, RuntimeScenario},
};

pub fn run(target: Option<CheckTarget>) -> Result<()> {
    let mut command = Command::new("cargo");
    command.args(["run", "--package", "dotfiles-checks", "--"]);
    match target {
        None => {}
        Some(CheckTarget::Static) => {
            command.arg("static");
        }
        Some(CheckTarget::Zsh) => {
            command.arg("zsh");
        }
        Some(CheckTarget::Runtime { scenario }) => match scenario {
            Some(RuntimeScenario::Full) | None => {
                command.arg("integration");
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
