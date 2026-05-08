use xshell::{Shell, cmd};

use crate::{Result, command::step, runtime, zsh};

pub fn run(args: Vec<String>) -> Result<()> {
    match args.as_slice() {
        [] => default_checks(),
        [target] if target == "static" => static_checks(),
        [target] if target == "zsh" => zsh::check(),
        [target] if target == "all" => all_checks(),
        [target, scenario] if target == "runtime" => runtime(scenario),
        _ => Err(format!("unsupported check arguments: {}", args.join(" ")).into()),
    }
}

pub fn static_checks() -> Result<()> {
    let shell = Shell::new()?;
    rust(&shell)?;
    nix(&shell)?;
    nix_diagnostics(&shell)?;
    runner_home(&shell)
}

fn default_checks() -> Result<()> {
    static_checks()?;
    zsh::check()
}

fn all_checks() -> Result<()> {
    default_checks()?;
    runtime("all")
}

fn rust(shell: &Shell) -> Result<()> {
    step("cargo fmt");
    cmd!(shell, "cargo fmt --all -- --check").run()?;
    step("cargo check");
    cmd!(shell, "env RUSTFLAGS='-D warnings' cargo check --workspace").run()?;
    step("cargo clippy");
    cmd!(
        shell,
        "cargo clippy --workspace --all-targets -- -D warnings"
    )
    .run()?;
    step("cargo test");
    cmd!(
        shell,
        "env RUSTFLAGS='-D warnings' cargo test --workspace --all-targets"
    )
    .run()?;
    Ok(())
}

fn nix(shell: &Shell) -> Result<()> {
    step("flake.lock exists");
    cmd!(shell, "test -s flake.lock").run()?;
    step("nix flake check");
    cmd!(
        shell,
        "nix --extra-experimental-features 'nix-command flakes' flake check --no-update-lock-file"
    )
    .run()?;
    let files = nix_files(shell)?;
    if !files.is_empty() {
        step("nix fmt");
        cmd!(
            shell,
            "nix --extra-experimental-features 'nix-command flakes' fmt -- --check {files...}"
        )
        .run()?;
    }
    Ok(())
}

fn nix_diagnostics(shell: &Shell) -> Result<()> {
    let files = nix_files(shell)?;
    if files.is_empty() {
        return Ok(());
    }

    step("nil diagnostics");
    let mut diagnostics = Vec::new();
    for file in files {
        let output = cmd!(shell, "nil diagnostics {file}").read()?;
        if !output.trim().is_empty() {
            diagnostics.push(format!("{file}:\n{output}"));
        }
    }
    if !diagnostics.is_empty() {
        return Err(format!(
            "nix diagnostics reported issues:\n{}",
            diagnostics.join("\n")
        )
        .into());
    }
    Ok(())
}

fn runner_home(shell: &Shell) -> Result<()> {
    step("runner Home Manager output eval");
    cmd!(
        shell,
        "nix --extra-experimental-features 'nix-command flakes' eval --no-update-lock-file .#homeConfigurations.runner.activationPackage.drvPath"
    )
    .run()?;
    Ok(())
}

fn runtime(scenario: &str) -> Result<()> {
    match scenario {
        "fresh-bootstrap" | "second-user-home-manager" | "darwin-switch-ya" | "all" => {}
        _ => return Err(format!("unsupported runtime scenario: {scenario}").into()),
    }

    runtime::run(scenario)
}

fn nix_files(shell: &Shell) -> Result<Vec<String>> {
    Ok(cmd!(shell, "git ls-files '*.nix'")
        .read()?
        .lines()
        .map(ToOwned::to_owned)
        .collect())
}
