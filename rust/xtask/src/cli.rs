use clap::{Parser, Subcommand, ValueEnum};

use crate::{Result, apply, check};

#[derive(Parser)]
#[command(name = "xtask")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Apply {
        #[arg(value_enum, default_value_t = ApplyTarget::All)]
        target: ApplyTarget,
    },
    Check {
        #[command(subcommand)]
        target: Option<CheckTarget>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum ApplyTarget {
    All,
    HomeManager,
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum RuntimeScenario {
    Full,
}

#[derive(Subcommand)]
pub(crate) enum CheckTarget {
    Static,
    Zsh,
    Runtime {
        #[arg(value_enum)]
        scenario: Option<RuntimeScenario>,
    },
    All,
}

pub fn dispatch() -> Result<()> {
    match Cli::parse().command {
        Command::Apply { target } => apply::run(target),
        Command::Check { target } => check::run(target),
    }
}
