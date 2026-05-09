use std::ffi::OsString;
use std::process::Command;

use crate::Result;
use anyhow::bail;
use dotfiles_core::command;

pub(crate) fn run<I>(program: impl Into<OsString>, args: I, dry_run: bool) -> Result<()>
where
    I: IntoIterator<Item = OsString>,
{
    let program = program.into();
    let args = args.into_iter().collect::<Vec<_>>();
    println!("$ {}", command::display(&program, &args));
    if dry_run {
        return Ok(());
    }

    let status = Command::new(&program).args(&args).status()?;
    if status.success() {
        Ok(())
    } else {
        bail!("command failed: {}: {status}", program.to_string_lossy())
    }
}
