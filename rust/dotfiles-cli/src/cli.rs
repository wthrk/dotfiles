use clap::{Parser, Subcommand};

use crate::Result;

#[derive(Parser)]
#[command(name = "dotfiles")]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init(crate::init::InitOptions),
    Switch(crate::switch::SwitchOptions),
}

pub(crate) fn dispatch() -> Result<()> {
    match Cli::parse().command {
        Command::Init(options) => crate::init::run(options),
        Command::Switch(options) => crate::switch::run(options),
    }
}
