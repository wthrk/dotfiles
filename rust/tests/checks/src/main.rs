use std::{env, fs, path::PathBuf, process};

use anyhow::bail;
use clap::{Parser, Subcommand, ValueEnum};
use xshell::{Shell, cmd};

mod command;
mod zsh;

use command::step;

type Result<T> = dotfiles_core::Result<T>;

#[derive(Parser)]
#[command(name = "dotfiles-checks")]
struct Cli {
    #[command(subcommand)]
    target: Option<CheckTarget>,
}

#[derive(Subcommand)]
enum CheckTarget {
    Static,
    Zsh,
    Integration {
        #[arg(value_enum)]
        scenario: Option<RuntimeScenario>,
    },
    All,
}

#[derive(Clone, Copy, ValueEnum)]
enum RuntimeScenario {
    Full,
}

fn main() -> std::process::ExitCode {
    match run(Cli::parse().target) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(target: Option<CheckTarget>) -> Result<()> {
    match target {
        None => default_checks(),
        Some(CheckTarget::Static) => static_checks(),
        Some(CheckTarget::Zsh) => zsh::check(),
        Some(CheckTarget::All) => all_checks(),
        Some(CheckTarget::Integration { scenario }) => {
            integration(scenario.unwrap_or(RuntimeScenario::Full))
        }
    }
}

fn static_checks() -> Result<()> {
    let shell = Shell::new()?;
    rust(&shell)?;
    nix(&shell)?;
    nix_diagnostics(&shell)?;
    runner_home(&shell)?;
    exported_modules(&shell)
}

fn default_checks() -> Result<()> {
    static_checks()?;
    zsh::check()
}

fn all_checks() -> Result<()> {
    default_checks()?;
    integration(RuntimeScenario::Full)
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
    cmd!(shell, "nix flake check --no-update-lock-file").run()?;
    let files = nix_files(shell)?;
    if !files.is_empty() {
        step("nix fmt");
        cmd!(shell, "nix fmt -- --check {files...}").run()?;
    }
    Ok(())
}

fn nix_diagnostics(shell: &Shell) -> Result<()> {
    let files = nix_files(shell)?;
    if files.is_empty() {
        return Ok(());
    }
    let nil = cmd!(shell, "command -v nil").ignore_status().read()?;
    if nil.trim().is_empty() {
        step("nil diagnostics skipped (nil not found)");
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
        bail!(
            "nix diagnostics reported issues:\n{}",
            diagnostics.join("\n")
        );
    }
    Ok(())
}

fn runner_home(shell: &Shell) -> Result<()> {
    let config_dir = TempDir::new("dotfiles-check")?;
    let config_dir_path = config_dir.path().display().to_string();
    let source = env::current_dir()?.canonicalize()?.display().to_string();

    step("dotfiles init output");
    cmd!(
        shell,
        "env DOTFILES_CONFIG_DIR={config_dir_path} cargo run --package dotfiles-cli -- init --user runner --host runner --system aarch64-darwin --source {source}"
    )
    .run()?;

    step("runner Home Manager output eval");
    cmd!(
        shell,
        "nix eval --no-update-lock-file {config_dir_path}#homeConfigurations.runner.activationPackage.drvPath"
    )
    .run()?;
    Ok(())
}

fn exported_modules(shell: &Shell) -> Result<()> {
    let config_dir = TempDir::new("dotfiles-module-check")?;
    let config_dir_path = config_dir.path().display().to_string();
    let source = env::current_dir()?.canonicalize()?.display().to_string();
    fs::write(
        config_dir.path().join("flake.nix"),
        external_module_flake(&source),
    )?;

    step("exported module flake lock");
    cmd!(shell, "nix flake lock {config_dir_path}").run()?;

    step("exported Home Manager module eval");
    cmd!(
        shell,
        "nix eval --no-update-lock-file {config_dir_path}#homeConfigurations.runner.activationPackage.drvPath"
    )
    .run()?;

    step("exported nix-darwin module eval");
    cmd!(
        shell,
        "nix eval --no-update-lock-file {config_dir_path}#darwinConfigurations.runner.system"
    )
    .run()?;
    Ok(())
}

fn external_module_flake(source: &str) -> String {
    format!(
        r#"{{
  inputs = {{
    dotfiles.url = "path:{source}";
    nixpkgs.follows = "dotfiles/nixpkgs";
    home-manager.follows = "dotfiles/home-manager";
    darwin.follows = "dotfiles/darwin";
  }};

  outputs = {{ dotfiles, nixpkgs, home-manager, darwin, ... }}:
    let
      system = "aarch64-darwin";
      pkgs = import nixpkgs {{ inherit system; config.allowUnfree = true; }};
    in {{
      homeConfigurations.runner = home-manager.lib.homeManagerConfiguration {{
        inherit pkgs;
        modules = [
          dotfiles.homeManagerModules.default
          {{ dotfiles.user = "runner"; }}
        ];
      }};

      darwinConfigurations.runner = darwin.lib.darwinSystem {{
        inherit system;
        modules = [
          dotfiles.darwinModules.default
          {{
            dotfiles = {{
              user = "runner";
              host = "runner";
            }};
          }}
        ];
      }};
    }};
}}
"#,
        source = source
    )
}

fn integration(scenario: RuntimeScenario) -> Result<()> {
    let shell = Shell::new()?;
    match scenario {
        RuntimeScenario::Full => {
            cmd!(shell, "cargo run --package dotfiles-integration-tests").run()?;
        }
    }
    Ok(())
}

fn nix_files(shell: &Shell) -> Result<Vec<String>> {
    Ok(cmd!(
        shell,
        "find . -path ./target -prune -o -name '*.nix' -type f -print"
    )
    .read()?
    .lines()
    .map(|path| path.trim_start_matches("./"))
    .map(ToOwned::to_owned)
    .collect())
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Result<Self> {
        let path = env::temp_dir().join(format!("{prefix}-{}", process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
