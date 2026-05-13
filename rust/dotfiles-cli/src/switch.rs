//! `dotfiles switch` がローカル flake の出力を適用する処理。
//!
//! Home Manager は `#<user>`、nix-darwin は `#<host>` を参照する。Darwin 適用前には
//! `/etc/bashrc` と `/etc/zshrc` が既存通常ファイルの場合だけ退避し、nix-darwin のリンク作成を妨げない。

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

/// 指定された対象を、生成済みローカル flake の属性名規約に従って適用する。
pub(crate) fn run(options: SwitchOptions) -> Result<()> {
    let config_dir = options.config_dir()?;
    ensure_config_exists(&config_dir)?;

    match options.target() {
        SwitchTarget::Home => switch_home(&config_dir, &options),
        SwitchTarget::Darwin => switch_darwin(&config_dir, &options),
        SwitchTarget::All => {
            switch_home(&config_dir, &options)?;
            switch_darwin(&config_dir, &options)
        }
    }
}

/// 既定または明示された設定ディレクトリに、適用対象の flake が存在することを確認する。
pub(crate) fn ensure_config_exists(config_dir: &Path) -> Result<()> {
    let config_path = config_dir.join("flake.nix");
    if !config_path.is_file() {
        bail!(
            "{} is missing; run `dotfiles init` first",
            config_path.display()
        );
    }

    Ok(())
}

/// `home-manager switch --flake <config-dir>#<user>` を実行する。
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

/// `sudo darwin-rebuild switch --flake <config-dir>#<host>` を実行する。
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

/// nix-darwin が `/etc/static` リンクを作る前に、衝突する既存シェル起動ファイルだけを退避する。
fn prepare_nix_darwin_etc(dry_run: bool) -> Result<()> {
    if std::env::consts::OS != "macos" {
        return Ok(());
    }

    for path in [Path::new("/etc/bashrc"), Path::new("/etc/zshrc")] {
        move_etc_file_before_nix_darwin(path, dry_run)?;
    }

    Ok(())
}

/// 管理済みリンクは触らず、通常ファイルだけを `<name>.before-nix-darwin` へ移動する。
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

/// `/etc/static`、`/run/current-system`、`/nix/store` へのリンクなら管理済みとみなす。
fn is_nix_darwin_etc_link(path: &Path) -> Result<bool> {
    let target = fs::read_link(path)?;
    Ok(target.starts_with("/etc/static")
        || target.starts_with("/run/current-system")
        || target.starts_with("/nix/store"))
}

/// CLI が受け取った設定ディレクトリをそのまま使い、ホームパスを推測しない。
fn flake_ref(path: &Path, output: &str) -> OsString {
    OsString::from(format!("{}#{}", path.display(), output))
}

#[derive(Args)]
/// 適用対象、出力名の上書き、外部コマンドのパス、予行実行を受け取る。
pub(crate) struct SwitchOptions {
    target: Option<SwitchTarget>,
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

impl SwitchOptions {
    /// `switch` と `update` が同じ設定ディレクトリ解決を使うための入口。
    pub(crate) fn config_dir(&self) -> Result<PathBuf> {
        config_dir(self.config_dir.clone())
    }

    /// 対象省略時は、日常利用で期待する Home Manager と Darwin の両方を適用する。
    fn target(&self) -> SwitchTarget {
        self.target.unwrap_or(SwitchTarget::All)
    }

    /// `update` が lock 更新と switch の両方を同じ予行実行モードで扱う。
    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }
}

#[derive(Clone, Copy, ValueEnum)]
/// `home` と `darwin` は独立して実行でき、`all` は Home Manager の後に Darwin を実行する。
enum SwitchTarget {
    Home,
    Darwin,
    All,
}
