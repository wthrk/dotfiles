use std::process::ExitCode;

mod apply;
mod check;
mod cli;

type Result<T> = dotfiles_core::Result<T>;

fn main() -> ExitCode {
    match cli::dispatch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}
