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
    let home_user = if switch_order(target).contains(&SwitchTarget::Home) {
        Some(HomeApplyUser::resolve(
            options.user.clone(),
            is_effective_root(),
        )?)
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
        user: home_user.as_ref().map_or("", HomeApplyUser::name),
        host: host.as_deref().unwrap_or(""),
        downgrade_to: home_user.as_ref().and_then(HomeApplyUser::downgrade_target),
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
/// 降格の要否は [`HomeApplyUser`] が決める。root で降格対象が無い状態はその型が構築を拒むため、
/// ここでは降格対象の有無だけを見る。
fn home_manager_invocation(
    config_dir: &Path,
    user: &str,
    downgrade_to: Option<&str>,
    home_manager: &OsString,
) -> SwitchInvocation {
    let args = [
        OsString::from("switch"),
        OsString::from("--flake"),
        flake_ref(config_dir, user),
    ];
    if let Some(target_user) = downgrade_to {
        SwitchInvocation {
            target: SwitchTarget::Home,
            program: OsString::from("sudo"),
            args: sudo_as_user_args(target_user, home_manager.clone(), args),
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
                input.downgrade_to,
                input.home_manager,
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
    /// root から降格して Home Manager を走らせる対象。降格しないなら `None`。
    /// 値は [`HomeApplyUser::downgrade_target`] が決める。
    downgrade_to: Option<&'a str>,
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
    /// 明示 `--user` がある場合だけ root から対象ユーザーへ降格する。
    pub(crate) fn root_user_override(&self) -> Option<&str> {
        if is_effective_root() {
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

/// Home Manager を適用する対象ユーザー。
///
/// root 実行で `--user` を省略した状態はこの型を構築できない。省略を現在ユーザー（root）へ倒すと Home Manager
/// が root のまま走り、利用者所有ファイルの所有者が root へ変わる。その状態を表現できなくすることで、
/// 呼び出し側が降格を落としても無言で通ることはなくなる。
#[derive(Debug)]
pub(crate) struct HomeApplyUser {
    name: String,
    /// root からこのユーザーへ降格して実行するか。
    downgrade_from_root: bool,
}

impl HomeApplyUser {
    /// 明示指定と実行時 euid から対象ユーザーを決める。root かつ未指定は `Err`。
    pub(crate) fn resolve(explicit: Option<String>, is_root: bool) -> Result<Self> {
        match (is_root, explicit) {
            (true, None) => bail!(
                "root で Home Manager を適用するには `--user` が必要（省略すると利用者所有ファイルが root 所有になる）"
            ),
            (true, Some(name)) => Ok(Self {
                name,
                downgrade_from_root: true,
            }),
            (false, explicit) => Ok(Self {
                name: explicit.map_or_else(current_user, Ok)?,
                downgrade_from_root: false,
            }),
        }
    }

    /// `#<user>` として flake 属性名に使う対象ユーザー名。
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// root から降格する場合の対象ユーザー。降格しないなら `None`。
    pub(crate) fn downgrade_target(&self) -> Option<&str> {
        self.downgrade_from_root.then_some(self.name.as_str())
    }
}

/// 実行時の euid を root 判定へ正規化する。
fn is_effective_root() -> bool {
    rustix::process::geteuid().is_root()
}

/// `darwin_rebuild_invocation` が euid に応じて sudo 前置の有無を切り替えることを検証する。
#[cfg(test)]
mod tests {
    use super::{
        HomeApplyUser, SwitchInvocationInput, SwitchTarget, darwin_rebuild_invocation,
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

    /// root 実行で `--user` を省略した状態は構築できない。降格が落ちたまま Home Manager が root で走る
    /// argv を組み立てられないことを、型の構築側で固定する。
    #[test]
    fn root_without_explicit_user_cannot_be_resolved() {
        let err = HomeApplyUser::resolve(None, true)
            .expect_err("root で --user 省略は構築できない")
            .to_string();
        assert!(err.contains("--user"), "{err}");
    }

    /// root で `--user` を明示した場合だけ、その利用者へ降格する。
    #[test]
    fn root_with_explicit_user_downgrades_to_that_user() -> anyhow::Result<()> {
        let resolved = HomeApplyUser::resolve(Some("alice".to_string()), true)?;
        assert_eq!(resolved.name(), "alice");
        assert_eq!(resolved.downgrade_target(), Some("alice"));
        Ok(())
    }

    /// 非 root では降格しない。
    #[test]
    fn non_root_does_not_downgrade() -> anyhow::Result<()> {
        let resolved = HomeApplyUser::resolve(Some("alice".to_string()), false)?;
        assert_eq!(resolved.name(), "alice");
        assert_eq!(resolved.downgrade_target(), None);
        Ok(())
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
            downgrade_to: None,
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
