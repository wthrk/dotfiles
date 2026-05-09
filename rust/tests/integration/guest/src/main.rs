use std::process::ExitCode;

use clap::Parser;
use scenario::RuntimeScenario;

mod assertions;
mod command;
mod runtime_env;
mod scenario;
mod users;

type Result<T> = dotfiles_core::Result<T>;

#[derive(Parser)]
#[command(name = "dotfiles-integration-test-guest")]
struct Args {
    #[arg(value_enum)]
    scenario: Option<RuntimeScenario>,
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<()> {
    scenario::ScenarioRunner::new()?.run_scenario(args.scenario.unwrap_or(RuntimeScenario::Full))
}
