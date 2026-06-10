//! `dotfiles switch` がローカル flake の出力を適用する処理。
//!
//! Home Manager は `#<user>`、nix-darwin は `#<host>` を参照する。Darwin 適用前には
//! `/etc/bashrc` と `/etc/zshrc` について、nix-darwin 管理リンク以外があれば退避し、
//! nix-darwin のリンク作成を妨げない。

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

/// `darwin-rebuild switch --flake <config-dir>#<host>` を、実行 euid に応じて適用する。
///
/// root（auto-update daemon の launchd 実行）では sudo を前置せず直接適用し、無人実行で sudo の
/// 対話/sudoers を要さない。非 root（対話利用者）では従来どおり `sudo` を前置して昇格する。
fn switch_darwin(config_dir: &Path, options: &SwitchOptions) -> Result<()> {
    let host = options.host.clone().map_or_else(current_host, Ok)?;
    prepare_nix_darwin_etc(options.dry_run)?;
    let invocation = darwin_rebuild_invocation(
        &options.darwin_rebuild,
        flake_ref(config_dir, &host),
        rustix::process::geteuid().is_root(),
    );
    run_process(invocation.program, invocation.args, options.dry_run)
}

/// `darwin-rebuild switch --flake <ref>` の実行プログラムと引数を、root 実行かどうかで決める。
///
/// root のときは `darwin-rebuild` を直接、非 root のときは `sudo` 経由で昇格する純粋関数で、
/// euid を引数で受け取り副作用を持たない（呼び出し側で euid を解決する）。
fn darwin_rebuild_invocation(
    darwin_rebuild: &OsString,
    flake_ref: OsString,
    is_root: bool,
) -> DarwinRebuildInvocation {
    let switch_args = [
        OsString::from("switch"),
        OsString::from("--flake"),
        flake_ref,
    ];
    if is_root {
        DarwinRebuildInvocation {
            program: darwin_rebuild.clone(),
            args: switch_args.into_iter().collect(),
        }
    } else {
        DarwinRebuildInvocation {
            program: OsString::from("sudo"),
            args: std::iter::once(darwin_rebuild.clone())
                .chain(switch_args)
                .collect(),
        }
    }
}

/// `darwin-rebuild switch` 実行の起動プログラムと引数列。
struct DarwinRebuildInvocation {
    program: OsString,
    args: Vec<OsString>,
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

/// 管理済みリンクは触らず、それ以外（通常ファイル・未管理シンボリックリンク）を
/// `<name>.before-nix-darwin` へ移動する。
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

/// nix-darwin が管理する代表的なリンク先（`/etc/static`、`/run/current-system`、`/nix/store`）なら管理済みとみなす。
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

/// `darwin_rebuild_invocation` が euid に応じて sudo 前置の有無を切り替えることを検証する。
#[cfg(test)]
mod tests {
    use super::darwin_rebuild_invocation;
    use std::ffi::OsString;

    /// root 実行では sudo を前置せず `darwin-rebuild switch` を直接起動する。
    #[test]
    fn root_invocation_runs_darwin_rebuild_without_sudo() {
        let invocation = darwin_rebuild_invocation(
            &OsString::from("darwin-rebuild"),
            OsString::from("/cfg#host"),
            true,
        );

        assert_eq!(invocation.program, OsString::from("darwin-rebuild"));
        assert_eq!(
            invocation.args,
            vec![
                OsString::from("switch"),
                OsString::from("--flake"),
                OsString::from("/cfg#host"),
            ]
        );
    }

    /// 非 root 実行では `sudo` を前置して `darwin-rebuild switch` を昇格起動する。
    #[test]
    fn non_root_invocation_prefixes_sudo() {
        let invocation = darwin_rebuild_invocation(
            &OsString::from("darwin-rebuild"),
            OsString::from("/cfg#host"),
            false,
        );

        assert_eq!(invocation.program, OsString::from("sudo"));
        assert_eq!(
            invocation.args,
            vec![
                OsString::from("darwin-rebuild"),
                OsString::from("switch"),
                OsString::from("--flake"),
                OsString::from("/cfg#host"),
            ]
        );
    }
}
