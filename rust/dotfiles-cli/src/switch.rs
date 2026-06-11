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
    process::{run as run_process, sudo_as_user_args},
};

/// 指定された対象を、生成済みローカル flake の属性名規約に従って適用する。
pub(crate) fn run(options: SwitchOptions) -> Result<()> {
    let config_dir = options.config_dir()?;
    ensure_config_exists(&config_dir)?;
    let target = options.target();
    let user = if switch_order(target).contains(&SwitchTarget::Home) {
        Some(options.user.clone().map_or_else(current_user, Ok)?)
    } else {
        None
    };
    let host = if switch_order(target).contains(&SwitchTarget::Darwin) {
        Some(options.host.clone().map_or_else(current_host, Ok)?)
    } else {
        None
    };
    let invocations = switch_invocations(SwitchInvocationInput {
        target,
        config_dir: &config_dir,
        user: user.as_deref().unwrap_or(""),
        host: host.as_deref().unwrap_or(""),
        user_override: options.user.as_deref(),
        home_manager: &options.home_manager,
        darwin_rebuild: &options.darwin_rebuild,
        is_root: is_effective_root(),
    });

    for invocation in invocations {
        if invocation.target == SwitchTarget::Darwin {
            prepare_nix_darwin_etc(options.dry_run)?;
        }
        run_process(invocation.program, invocation.args, options.dry_run)?;
    }
    Ok(())
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

/// `home-manager switch --flake <config-dir>#<user>` の実行プログラムと引数を組み立てる。
fn home_manager_invocation(
    config_dir: &Path,
    user: &str,
    user_override: Option<&str>,
    home_manager: &OsString,
    is_root: bool,
) -> SwitchInvocation {
    let args = [
        OsString::from("switch"),
        OsString::from("--flake"),
        flake_ref(config_dir, user),
    ];
    if should_run_as_target_user(is_root, user_override) {
        SwitchInvocation {
            target: SwitchTarget::Home,
            program: OsString::from("sudo"),
            args: sudo_as_user_args(user, home_manager.clone(), args),
        }
    } else {
        SwitchInvocation {
            target: SwitchTarget::Home,
            program: home_manager.clone(),
            args: args.into_iter().collect(),
        }
    }
}

/// `darwin-rebuild switch --flake <ref>` の実行プログラムと引数を、root 実行かどうかで決める。
///
/// root のときは `darwin-rebuild` を直接、非 root のときは `sudo` 経由で昇格する純粋関数で、
/// euid を引数で受け取り副作用を持たない（呼び出し側で euid を解決する）。
fn darwin_rebuild_invocation(
    darwin_rebuild: &OsString,
    flake_ref: OsString,
    is_root: bool,
) -> SwitchInvocation {
    let switch_args = [
        OsString::from("switch"),
        OsString::from("--flake"),
        flake_ref,
    ];
    if is_root {
        SwitchInvocation {
            target: SwitchTarget::Darwin,
            program: darwin_rebuild.clone(),
            args: switch_args.into_iter().collect(),
        }
    } else {
        SwitchInvocation {
            target: SwitchTarget::Darwin,
            program: OsString::from("sudo"),
            args: std::iter::once(darwin_rebuild.clone())
                .chain(switch_args)
                .collect(),
        }
    }
}

/// `dotfiles switch` が実行する外部コマンド列を副作用なしで組み立てる。
fn switch_invocations(input: SwitchInvocationInput<'_>) -> Vec<SwitchInvocation> {
    switch_order(input.target)
        .iter()
        .map(|target| match target {
            SwitchTarget::Home => home_manager_invocation(
                input.config_dir,
                input.user,
                input.user_override,
                input.home_manager,
                input.is_root,
            ),
            SwitchTarget::Darwin => darwin_rebuild_invocation(
                input.darwin_rebuild,
                flake_ref(input.config_dir, input.host),
                input.is_root,
            ),
            SwitchTarget::All => unreachable!("SwitchTarget::All is expanded before execution"),
        })
        .collect()
}

struct SwitchInvocationInput<'a> {
    target: SwitchTarget,
    config_dir: &'a Path,
    user: &'a str,
    host: &'a str,
    user_override: Option<&'a str>,
    home_manager: &'a OsString,
    darwin_rebuild: &'a OsString,
    is_root: bool,
}

/// `dotfiles switch` 実行の起動プログラムと引数列。
struct SwitchInvocation {
    target: SwitchTarget,
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

    /// root 実行時に利用者所有ファイルを更新する対象ユーザーを返す。
    ///
    /// 明示 `--user` がある場合だけ root から対象ユーザーへ降格する。通常の root shell で `--user` を
    /// 指定しない実行は従来どおり現在ユーザー（root）を対象にする。
    pub(crate) fn root_user_override(&self) -> Option<&str> {
        if should_run_as_target_user(is_effective_root(), self.user.as_deref()) {
            self.user.as_deref()
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
/// `home` と `darwin` は独立して実行でき、`all` は Home Manager の後に Darwin を実行する。
enum SwitchTarget {
    Home,
    Darwin,
    All,
}

/// `all` を Home Manager -> Darwin の適用順序へ展開する。
fn switch_order(target: SwitchTarget) -> &'static [SwitchTarget] {
    match target {
        SwitchTarget::Home => &[SwitchTarget::Home],
        SwitchTarget::Darwin => &[SwitchTarget::Darwin],
        SwitchTarget::All => &[SwitchTarget::Home, SwitchTarget::Darwin],
    }
}

/// root かつ対象ユーザーが明示されたときだけ、利用者所有の処理を対象ユーザー権限で実行する。
fn should_run_as_target_user(is_root: bool, user_override: Option<&str>) -> bool {
    is_root && user_override.is_some()
}

/// 実行時の euid を root 判定へ正規化する。
fn is_effective_root() -> bool {
    rustix::process::geteuid().is_root()
}

/// `darwin_rebuild_invocation` が euid に応じて sudo 前置の有無を切り替えることを検証する。
#[cfg(test)]
mod tests {
    use super::{
        SwitchInvocationInput, SwitchTarget, darwin_rebuild_invocation, should_run_as_target_user,
        switch_invocations, switch_order,
    };
    use std::ffi::OsString;
    use std::path::Path;

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

    /// root daemon が `--user` を明示したときだけ Home Manager と lock 更新を降格する。
    #[test]
    fn user_context_is_only_for_root_with_explicit_user() {
        assert!(should_run_as_target_user(true, Some("alice")));
        assert!(!should_run_as_target_user(true, None));
        assert!(!should_run_as_target_user(false, Some("alice")));
    }

    /// 既定 target の `all` は standalone Home Manager を先に適用してから nix-darwin を適用する。
    #[test]
    fn all_expands_to_home_manager_then_darwin() {
        assert_eq!(
            switch_order(SwitchTarget::All),
            &[SwitchTarget::Home, SwitchTarget::Darwin]
        );
    }

    /// `all` 経路が Home Manager を適用してから nix-darwin を適用するコマンド列を組み立てる。
    #[test]
    fn all_invocations_run_home_manager_then_darwin() {
        let home_manager = OsString::from("home-manager");
        let darwin_rebuild = OsString::from("darwin-rebuild");
        let invocations = switch_invocations(SwitchInvocationInput {
            target: SwitchTarget::All,
            config_dir: Path::new("/cfg"),
            user: "alice",
            host: "mac",
            user_override: Some("alice"),
            home_manager: &home_manager,
            darwin_rebuild: &darwin_rebuild,
            is_root: false,
        });

        assert_eq!(invocations.len(), 2);
        assert_eq!(invocations[0].program, OsString::from("home-manager"));
        assert_eq!(
            invocations[0].args,
            vec![
                OsString::from("switch"),
                OsString::from("--flake"),
                OsString::from("/cfg#alice"),
            ]
        );
        assert_eq!(invocations[1].program, OsString::from("sudo"));
        assert_eq!(
            invocations[1].args,
            vec![
                OsString::from("darwin-rebuild"),
                OsString::from("switch"),
                OsString::from("--flake"),
                OsString::from("/cfg#mac"),
            ]
        );
    }
}
