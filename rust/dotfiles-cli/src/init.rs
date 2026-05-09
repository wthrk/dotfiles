//! `dotfiles init` がユーザー所有のローカル flake を作る処理。
//!
//! 書き込む先は既定で `$HOME/.config/dotfiles/flake.nix`。生成後に `nix flake lock` を実行し、
//! 後続の `switch` が暗黙の入力更新をしない状態にする。

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

/// 既存ファイルは `--force` なしでは上書きせず、生成後に lock file を具体化する。
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

/// 生成先ディレクトリで `nix flake lock` を実行し、入力解決を明示的な lock file に固定する。
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
/// ローカル flake に記録するユーザー名、ホスト名、システム名、参照元を受け取る。
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
