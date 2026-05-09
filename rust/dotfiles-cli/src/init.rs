use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, bail};
use clap::Args;

use crate::{
    Result,
    environment::{config_path, current_host, current_user, default_system},
    local_flake,
    process::run as run_process,
};

const DEFAULT_SOURCE: &str = "github:wthrk/dotfiles";

pub(crate) fn run(options: InitOptions) -> Result<()> {
    let config_path = config_path(options.config_dir.clone())?;
    if config_path.exists() && !options.force {
        bail!(
            "{} already exists; pass --force to replace it",
            config_path.display()
        );
    }

    let user = options.user.unwrap_or(current_user()?);
    let host = options.host.unwrap_or(current_host()?);
    let system = options.system.unwrap_or_else(default_system);

    let config_dir = config_path
        .parent()
        .ok_or_else(|| anyhow!("config path does not have a parent directory"))?;
    fs::create_dir_all(config_dir)?;
    fs::write(
        &config_path,
        local_flake::render(&options.source, &user, &host, &system),
    )?;
    lock_config(config_dir)?;

    println!("wrote {}", config_path.display());
    Ok(())
}

fn lock_config(config_dir: &std::path::Path) -> Result<()> {
    run_process(
        "nix",
        [
            OsString::from("flake"),
            OsString::from("lock"),
            config_dir.as_os_str().to_os_string(),
        ],
        false,
    )
}

#[derive(Args)]
pub(crate) struct InitOptions {
    #[arg(long, env = "DOTFILES_USER")]
    user: Option<String>,
    #[arg(long, env = "DOTFILES_HOST")]
    host: Option<String>,
    #[arg(long, env = "DOTFILES_SYSTEM")]
    system: Option<String>,
    #[arg(long, env = "DOTFILES_SOURCE", default_value = DEFAULT_SOURCE)]
    source: String,
    #[arg(long, env = "DOTFILES_CONFIG_DIR", value_name = "PATH")]
    config_dir: Option<PathBuf>,
    #[arg(long)]
    force: bool,
}
