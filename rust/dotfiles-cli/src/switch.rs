//! `dotfiles switch` がローカル flake の出力を適用する処理。
//!
//! Home Manager は `#<user>`、nix-darwin は `#<host>` を参照する。Darwin 適用前には
//! `/etc/bashrc` と `/etc/zshrc` について、nix-darwin 管理リンク以外があれば退避し、
//! nix-darwin のリンク作成を妨げない。
//!
//! Darwin 適用は通常 `sudo darwin-rebuild` で昇格するが、auto-update.nix の root daemon は
//! 既に root で動くため `--no-sudo`（または `DOTFILES_DARWIN_REBUILD_SUDO=0`）で sudo を前置しない
//! 経路を選ぶ。非 root でこの経路を選んだ場合は昇格しないので `darwin-rebuild` が失敗する（昇格なしの
//! 適用は失敗が正しい挙動であり、本 CLI は黙って成功扱いにしない）。

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
    apply(&options.common, options.target())
}

/// 共通オプションと適用対象から、生成済みローカル flake の属性名規約に従って適用する。
///
/// `update` は target を受け取らず常に [`SwitchTarget::All`] で呼ぶための入口でもある。`All` の実体は
/// プラットフォームで分かれ（[`targets_for_all`]）、macOS は darwin-rebuild 一本（home も一括適用）、
/// 非 macOS は home-manager standalone 一本になる。
pub(crate) fn apply(common: &SwitchCommon, target: SwitchTarget) -> Result<()> {
    let config_dir = common.config_dir()?;
    ensure_config_exists(&config_dir)?;

    match target {
        SwitchTarget::Home => switch_home(&config_dir, common),
        SwitchTarget::Darwin => switch_darwin(&config_dir, common),
        SwitchTarget::All => {
            for unit in targets_for_all(cfg!(target_os = "macos")) {
                match unit {
                    SwitchTarget::Home => switch_home(&config_dir, common)?,
                    SwitchTarget::Darwin => switch_darwin(&config_dir, common)?,
                    // `targets_for_all` は Home/Darwin の単一 unit 列だけを返すため All は到達しない。
                    SwitchTarget::All => {}
                }
            }
            Ok(())
        }
    }
}

/// `SwitchTarget::All`（`dotfiles update` と `switch all`）で実際に実行する単一 target 列を
/// プラットフォームから決める純粋関数。
///
/// macOS では home-manager が nix-darwin モジュール（`darwin.nix` の `home-manager.users.${user}`）として
/// 組み込まれ `darwin-rebuild switch` が system と home の両方を一度に適用する。よって standalone
/// `home-manager switch` は呼ばず `Darwin` のみを返す（standalone を追加で呼ぶと二重適用になり、root daemon
/// 経路では standalone home が `~/.local/state/home-manager` 等へ root 所有ファイルを残し以後の
/// ユーザ操作が EACCES で失敗する）。非 macOS（Linux 等）には darwin-rebuild が無いため `Home`（home-manager
/// standalone）のみを返す。`switch home`/`switch darwin` の明示単一 target はこの関数を経由せず従来どおり。
fn targets_for_all(is_macos: bool) -> &'static [SwitchTarget] {
    if is_macos {
        &[SwitchTarget::Darwin]
    } else {
        &[SwitchTarget::Home]
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
fn switch_home(config_dir: &Path, options: &SwitchCommon) -> Result<()> {
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

/// `[sudo] darwin-rebuild switch --flake <config-dir>#<host>` を実行する。
///
/// 既定では `sudo` で昇格するが、`--no-sudo`（または `DOTFILES_DARWIN_REBUILD_SUDO=0`）が指定された場合は
/// 既に root であることを前提に `darwin-rebuild` を直接呼ぶ（root daemon 経路）。退避処理（`mv`）の昇格有無も
/// 同じ判定に揃える。
fn switch_darwin(config_dir: &Path, options: &SwitchCommon) -> Result<()> {
    let host = options.host.clone().map_or_else(current_host, Ok)?;
    let use_sudo = options.use_sudo();
    prepare_nix_darwin_etc(use_sudo, options.dry_run)?;
    let (program, args) = darwin_rebuild_command(
        use_sudo,
        &options.darwin_rebuild,
        flake_ref(config_dir, &host),
    );
    run_process(program, args, options.dry_run)
}

/// `darwin-rebuild switch` を実行するための program 名と引数列を組み立てる純粋関数。
///
/// `use_sudo` が真なら program は `sudo`・引数先頭に `darwin-rebuild` を置いて昇格する。偽なら program を
/// `darwin-rebuild` 直呼びにして sudo を前置しない。実行から分離し、sudo 有無の引数列を単体検証できるようにする。
fn darwin_rebuild_command(
    use_sudo: bool,
    darwin_rebuild: &OsString,
    flake_ref: OsString,
) -> (OsString, Vec<OsString>) {
    let switch_args = [
        OsString::from("switch"),
        OsString::from("--flake"),
        flake_ref,
    ];
    if use_sudo {
        let args = std::iter::once(darwin_rebuild.clone())
            .chain(switch_args)
            .collect();
        (OsString::from("sudo"), args)
    } else {
        (darwin_rebuild.clone(), switch_args.into_iter().collect())
    }
}

/// `DOTFILES_DARWIN_REBUILD_SUDO` の値と `--no-sudo` フラグから、sudo を前置するかを決める純粋関数。
///
/// `--no-sudo` が指定されているか、env が `0` なら sudo を前置しない。それ以外（未設定/`0` 以外）は昇格する。
/// env と flag のどちらでも sudo 省略を要求できるようにし、判定を env 参照から切り離してテスト可能にする。
fn should_use_sudo(no_sudo_flag: bool, env_value: Option<&str>) -> bool {
    if no_sudo_flag {
        return false;
    }
    env_value != Some("0")
}

/// nix-darwin が `/etc/static` リンクを作る前に、衝突する既存シェル起動ファイルだけを退避する。
///
/// `use_sudo` は `darwin-rebuild` 呼び出しと同じ昇格方針を退避の `mv` にも適用する（root daemon 経路では
/// sudo を前置しない）。
fn prepare_nix_darwin_etc(use_sudo: bool, dry_run: bool) -> Result<()> {
    if std::env::consts::OS != "macos" {
        return Ok(());
    }

    for path in [Path::new("/etc/bashrc"), Path::new("/etc/zshrc")] {
        move_etc_file_before_nix_darwin(path, use_sudo, dry_run)?;
    }

    Ok(())
}

/// 管理済みリンクは触らず、それ以外（通常ファイル・未管理シンボリックリンク）を
/// `<name>.before-nix-darwin` へ移動する。
fn move_etc_file_before_nix_darwin(path: &Path, use_sudo: bool, dry_run: bool) -> Result<()> {
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
    let (program, args) = move_command(
        use_sudo,
        path.as_os_str().to_os_string(),
        backup.as_os_str().to_os_string(),
    );
    run_process(program, args, dry_run)
}

/// 退避用 `mv` の program 名と引数列を組み立てる純粋関数。
///
/// `use_sudo` が真なら `sudo mv ...`、偽なら `mv ...` を直接呼ぶ。`darwin-rebuild` と同じ昇格方針を退避にも
/// 適用するための分離点で、sudo 有無の引数列を単体検証できるようにする。
fn move_command(use_sudo: bool, from: OsString, to: OsString) -> (OsString, Vec<OsString>) {
    if use_sudo {
        (OsString::from("sudo"), vec![OsString::from("mv"), from, to])
    } else {
        (OsString::from("mv"), vec![from, to])
    }
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

#[derive(Args, Clone)]
/// `dotfiles switch` の利用者オプション。適用対象と共通オプションを受け取る。
///
/// `target` は `switch` だけが受け取る。`update` は常に全体適用（home+darwin）で部分 target を受理しないため、
/// `target` を持たない [`SwitchCommon`] だけを flatten する（部分適用後に全体 marker を確定する不整合を防ぐ）。
pub(crate) struct SwitchOptions {
    target: Option<SwitchTarget>,
    #[command(flatten)]
    common: SwitchCommon,
}

#[derive(Args, Clone)]
/// 出力名の上書き、外部コマンドのパス、sudo 省略、予行実行といった `switch`/`update` 共通オプション。
///
/// 適用対象（`target`）は含めない。`update` はこれを flatten し常に全体適用するため、ここに target を置くと
/// `dotfiles update home` を受理してしまう。
pub(crate) struct SwitchCommon {
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
    /// 既に root で実行しているときに `darwin-rebuild` を sudo 無しで呼ぶ（root daemon 経路）。
    ///
    /// 非 root でこれを指定すると昇格しないので `darwin-rebuild` 自体が失敗する（その失敗が正しい）。env
    /// `DOTFILES_DARWIN_REBUILD_SUDO=0` でも同じ経路を選べる。
    #[arg(long)]
    no_sudo: bool,
    #[arg(long)]
    dry_run: bool,
}

/// `DOTFILES_DARWIN_REBUILD_SUDO=0` で sudo 省略を要求するための env 名。
const DARWIN_REBUILD_SUDO_ENV: &str = "DOTFILES_DARWIN_REBUILD_SUDO";

impl SwitchOptions {
    /// 対象省略時は [`SwitchTarget::All`]。実体はプラットフォームで分かれ（[`targets_for_all`]）、
    /// macOS は darwin-rebuild 一本（home も一括適用）、非 macOS は home-manager standalone 一本になる。
    fn target(&self) -> SwitchTarget {
        self.target.unwrap_or(SwitchTarget::All)
    }
}

impl SwitchCommon {
    /// `switch` と `update` が同じ設定ディレクトリ解決を使うための入口。
    pub(crate) fn config_dir(&self) -> Result<PathBuf> {
        config_dir(self.config_dir.clone())
    }

    /// `update` が lock 更新と switch の両方を同じ予行実行モードで扱う。
    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }

    /// Darwin 適用で `sudo` を前置するか。`--no-sudo` か env `DOTFILES_DARWIN_REBUILD_SUDO=0` で省略する。
    fn use_sudo(&self) -> bool {
        let env_value = std::env::var(DARWIN_REBUILD_SUDO_ENV).ok();
        should_use_sudo(self.no_sudo, env_value.as_deref())
    }
}

#[derive(Clone, Copy, ValueEnum)]
/// `home` と `darwin` は独立して実行できる。`all` はプラットフォームに応じて単一経路を選ぶ
/// （[`targets_for_all`] 参照。macOS は darwin-rebuild が home も一括適用するので Darwin のみ、
/// 非 macOS は home-manager standalone のみ）。
pub(crate) enum SwitchTarget {
    Home,
    Darwin,
    All,
}

#[cfg(test)]
mod tests {
    //! Darwin 適用の sudo 有無で program 名と引数列が決まることを固定する。
    //! root daemon 経路（sudo 省略）と通常経路（sudo 昇格）の双方を引数列で検証する。
    //! あわせて `All` 展開がプラットフォームで分岐すること（macOS→darwin のみ・非 macOS→home のみ）を固定する。

    use std::ffi::OsString;

    use super::{
        SwitchTarget, darwin_rebuild_command, move_command, should_use_sudo, targets_for_all,
    };

    /// `SwitchTarget` は `PartialEq` を持たないので、All 展開を識別子で比較できるよう文字列へ写す。
    fn target_names(targets: &[SwitchTarget]) -> Vec<&'static str> {
        targets
            .iter()
            .map(|target| match target {
                SwitchTarget::Home => "home",
                SwitchTarget::Darwin => "darwin",
                SwitchTarget::All => "all",
            })
            .collect()
    }

    #[test]
    fn all_on_macos_runs_darwin_only() {
        // macOS では darwin-rebuild が home も一括適用するため、standalone home は呼ばず Darwin 一本。
        // standalone home を足すと二重適用になり root daemon 経路でユーザ home に root 所有ファイルを残す。
        assert_eq!(target_names(targets_for_all(true)), vec!["darwin"]);
    }

    #[test]
    fn all_on_non_macos_runs_home_only() {
        // 非 macOS（Linux 等）には darwin-rebuild が無いため home-manager standalone 一本。
        assert_eq!(target_names(targets_for_all(false)), vec!["home"]);
    }

    /// 引数列を比較しやすいよう `OsString` を文字列へ揃える。
    fn as_strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn darwin_rebuild_uses_sudo_by_default() {
        // 既定（sudo 昇格）では program が `sudo`、引数先頭に darwin-rebuild が来る。
        let (program, args) = darwin_rebuild_command(
            true,
            &OsString::from("darwin-rebuild"),
            OsString::from("/cfg#host"),
        );
        assert_eq!(program.to_string_lossy(), "sudo");
        assert_eq!(
            as_strings(&args),
            vec!["darwin-rebuild", "switch", "--flake", "/cfg#host"]
        );
    }

    #[test]
    fn darwin_rebuild_omits_sudo_when_requested() {
        // root daemon 経路（sudo 省略）では darwin-rebuild を直接呼び、sudo を前置しない。
        let (program, args) = darwin_rebuild_command(
            false,
            &OsString::from("darwin-rebuild"),
            OsString::from("/cfg#host"),
        );
        assert_eq!(program.to_string_lossy(), "darwin-rebuild");
        assert_eq!(as_strings(&args), vec!["switch", "--flake", "/cfg#host"]);
    }

    #[test]
    fn move_uses_sudo_by_default() {
        let (program, args) = move_command(
            true,
            OsString::from("/etc/zshrc"),
            OsString::from("/etc/zshrc.bak"),
        );
        assert_eq!(program.to_string_lossy(), "sudo");
        assert_eq!(
            as_strings(&args),
            vec!["mv", "/etc/zshrc", "/etc/zshrc.bak"]
        );
    }

    #[test]
    fn move_omits_sudo_when_requested() {
        let (program, args) = move_command(
            false,
            OsString::from("/etc/zshrc"),
            OsString::from("/etc/zshrc.bak"),
        );
        assert_eq!(program.to_string_lossy(), "mv");
        assert_eq!(as_strings(&args), vec!["/etc/zshrc", "/etc/zshrc.bak"]);
    }

    #[test]
    fn should_use_sudo_resolves_flag_and_env() {
        // 既定（flag なし・env なし）は昇格する。
        assert!(should_use_sudo(false, None));
        // env が `0` 以外なら昇格する。
        assert!(should_use_sudo(false, Some("1")));
        // `--no-sudo` flag は env に関わらず省略する。
        assert!(!should_use_sudo(true, None));
        assert!(!should_use_sudo(true, Some("1")));
        // env `0` は省略する。
        assert!(!should_use_sudo(false, Some("0")));
    }
}
