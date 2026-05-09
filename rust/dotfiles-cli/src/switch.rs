use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::bail;
use clap::{Args, ValueEnum};

use crate::{
    Result,
    environment::{config_dir, current_host, current_user},
    process::run as run_process,
};

pub(crate) fn run(options: SwitchOptions) -> Result<()> {
    let config_dir = config_dir(options.config_dir.clone())?;
    let config_path = config_dir.join("flake.nix");
    if !config_path.is_file() {
        bail!(
            "{} is missing; run `dotfiles init` first",
            config_path.display()
        );
    }

    match options.target {
        SwitchTarget::Home => switch_home(&config_dir, &options),
        SwitchTarget::Darwin => switch_darwin(&config_dir, &options),
        SwitchTarget::All => {
            switch_home(&config_dir, &options)?;
            switch_darwin(&config_dir, &options)
        }
    }
}

fn switch_home(config_dir: &Path, options: &SwitchOptions) -> Result<()> {
    let user = options.user.clone().map_or_else(current_user, Ok)?;
    run_process(
        options.home_manager.clone(),
        [
            OsString::from("switch"),
            OsString::from("--flake"),
            flake_ref(config_dir, &user),
        ],
        options.dry_run,
    )
}

fn switch_darwin(config_dir: &Path, options: &SwitchOptions) -> Result<()> {
    let host = options.host.clone().map_or_else(current_host, Ok)?;
    prepare_nix_darwin_etc(options.dry_run)?;
    run_process(
        "sudo",
        std::iter::once(options.darwin_rebuild.clone()).chain([
            OsString::from("switch"),
            OsString::from("--flake"),
            flake_ref(config_dir, &host),
        ]),
        options.dry_run,
    )
}

fn prepare_nix_darwin_etc(dry_run: bool) -> Result<()> {
    if std::env::consts::OS != "macos" {
        return Ok(());
    }

    for path in [Path::new("/etc/bashrc"), Path::new("/etc/zshrc")] {
        move_etc_file_before_nix_darwin(path, dry_run)?;
    }

    Ok(())
}

fn move_etc_file_before_nix_darwin(path: &Path, dry_run: bool) -> Result<()> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if metadata.file_type().is_symlink() && is_nix_darwin_etc_link(path)? {
        return Ok(());
    }

    let backup = PathBuf::from(format!("{}.before-nix-darwin", path.display()));
    if backup.exists() {
        bail!(
            "{} and {} both exist; move one aside before `dotfiles switch darwin`",
            path.display(),
            backup.display()
        );
    }

    println!(
        "nix-darwin 管理前に退避します: {} -> {}",
        path.display(),
        backup.display()
    );
    run_process(
        "sudo",
        [
            OsString::from("mv"),
            path.as_os_str().to_os_string(),
            backup.as_os_str().to_os_string(),
        ],
        dry_run,
    )
}

fn is_nix_darwin_etc_link(path: &Path) -> Result<bool> {
    let target = fs::read_link(path)?;
    Ok(target.starts_with("/etc/static")
        || target.starts_with("/run/current-system")
        || target.starts_with("/nix/store"))
}

fn flake_ref(path: &Path, output: &str) -> OsString {
    OsString::from(format!("{}#{}", path.display(), output))
}

#[derive(Args)]
pub(crate) struct SwitchOptions {
    target: SwitchTarget,
    #[arg(long, env = "DOTFILES_USER")]
    user: Option<String>,
    #[arg(long, env = "DOTFILES_HOST")]
    host: Option<String>,
    #[arg(long, env = "DOTFILES_CONFIG_DIR", value_name = "PATH")]
    config_dir: Option<PathBuf>,
    #[arg(long, env = "DOTFILES_HOME_MANAGER", default_value = "home-manager")]
    home_manager: OsString,
    #[arg(
        long,
        env = "DOTFILES_DARWIN_REBUILD",
        default_value = "darwin-rebuild"
    )]
    darwin_rebuild: OsString,
    #[arg(long)]
    dry_run: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum SwitchTarget {
    Home,
    Darwin,
    All,
}
