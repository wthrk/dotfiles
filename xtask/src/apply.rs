use xshell::{Shell, cmd};

use crate::{Result, check, command::step};

pub fn run(args: Vec<String>) -> Result<()> {
    match args.as_slice() {
        [] => all(),
        [target] if target == "all" => all(),
        [target] if target == "home-manager" => home_manager(),
        _ => Err(format!("unsupported apply arguments: {}", args.join(" ")).into()),
    }
}

fn all() -> Result<()> {
    check::static_checks()?;
    let shell = Shell::new()?;
    step("apply all");
    cmd!(shell, "sudo darwin-rebuild switch --flake .#default").run()?;
    Ok(())
}

fn home_manager() -> Result<()> {
    check::static_checks()?;
    let shell = Shell::new()?;
    step("home-manager switch");
    cmd!(shell, "home-manager switch --flake .#default").run()?;
    Ok(())
}
