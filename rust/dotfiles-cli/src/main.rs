use std::process::ExitCode;

mod cli;
mod environment;
mod init;
mod local_flake;
mod process;
mod switch;

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
