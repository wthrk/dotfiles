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
    environment::{
        ConfigScope, config_path, config_scope, current_host, current_user, default_system,
    },
    local_flake,
    process::run as run_process,
};

const DEFAULT_SOURCE: &str = "github:wthrk/dotfiles";

/// 既存ファイルは `--force` なしでは上書きせず、生成後に lock file を具体化する。
///
/// 生成する出力の範囲は利用者に指定させず、このマシンの system 層を誰が持っているかから決める。
/// これにより、導入手順はユーザーの種類で分かれない。
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
    let scope = config_scope(&user)?;

    let config_dir = config_path
        .parent()
        .ok_or_else(|| anyhow!("config path does not have a parent directory"))?;
    fs::create_dir_all(config_dir)?;
    fs::write(
        &config_path,
        local_flake::render(
            &options.source,
            &user,
            &host,
            &system,
            !options.skip_self_package,
            scope,
        ),
    )?;
    lock_config(config_dir)?;

    println!("wrote {} ({})", config_path.display(), scope_summary(scope));
    Ok(())
}

/// 生成した出力の範囲を、そう決まった理由と一緒に 1 行で伝える。
fn scope_summary(scope: ConfigScope) -> &'static str {
    match scope {
        ConfigScope::Full => "home 層と system 層",
        ConfigScope::Home => "home 層のみ: system 層はこのマシンの別ユーザーが管理している",
    }
}

/// 生成先ディレクトリで `nix flake lock` を実行し、dirty な local input も lock file に固定する。
fn lock_config(config_dir: &std::path::Path) -> Result<()> {
    run_process(
        "nix",
        [
            OsString::from("flake"),
            OsString::from("lock"),
            OsString::from("--allow-dirty-locks"),
            config_dir.as_os_str().to_os_string(),
        ],
        false,
        // `init` は利用者が起動する 1 回限りの処理で、無人走査の一部にならないため期限を置かない。
        None,
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
    skip_self_package: bool,
    #[arg(long)]
    force: bool,
}
