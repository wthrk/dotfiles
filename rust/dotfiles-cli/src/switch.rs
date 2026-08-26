//! `dotfiles switch` がローカル flake の出力を適用する処理。
//!
//! Home Manager は `#<user>`、nix-darwin は `#<host>` を参照する。Darwin 適用前には
//! `/etc/bashrc` と `/etc/zshrc` について、nix-darwin 管理リンク以外があれば退避し、
//! nix-darwin のリンク作成を妨げない。

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::bail;
use clap::{Args, ValueEnum};

use crate::{
    Result,
    environment::{ConfigScope, config_dir, config_scope, current_host, current_user},
    process::{Invocation, run as run_process},
};

/// 指定された対象を、生成済みローカル flake の属性名規約に従って適用する。
///
/// `deadline` は無人実行で 1 対象に与える打ち切り時刻で、外部コマンドの実行へそのまま渡す。利用者
/// 自身の実行は `None` で呼び、途中のビルドを打ち切らない。
///
/// 戻り値は実際に適用した層を適用順で並べたものである。適用後の後始末（世代の掃除）は層ごとに
/// 必要な権限が違うため、呼び出し側はこの戻り値で対象を選ぶ。ここと別の規則で層を判定すると、
/// 適用していない層の後始末を走らせることになる。
pub(crate) fn run(
    options: SwitchOptions,
    deadline: Option<Instant>,
) -> Result<&'static [SwitchTarget]> {
    let config_dir = options.config_dir()?;
    ensure_config_exists(&config_dir)?;
    let applied = planned_targets(&options)?;
    // 利用者所有ファイルを書く Home 適用を含むときだけ対象ユーザーを要求する。降格対象は最初の
    // 特権コマンドより前に解決する。Darwin 単独 target は利用者所有ファイルを書かない。
    let home_user = applied
        .contains(&SwitchTarget::Home)
        .then(|| options.home_apply_user())
        .transpose()?;
    let host = if applied.contains(&SwitchTarget::Darwin) {
        Some(options.host()?)
    } else {
        None
    };
    let invocations = switch_invocations(SwitchInvocationInput {
        targets: applied,
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
            prepare_nix_darwin_etc(options.dry_run, deadline)?;
        }
        invocation.command.run(options.dry_run, deadline)?;
    }
    Ok(applied)
}

/// この options が適用する層を、適用順で返す。
///
/// 適用範囲は対象ユーザーがこのマシンで持つ層から決まる。scope はそのユーザーに紐づくので、対象
/// ユーザーが決まらない実行では解決しない。`update` は適用の前にこの層を知る必要があるため、
/// [`run`] と同じ解決をここから共有する。層の決め方が 2 か所に分かれると、適用する層と、適用前に
/// 調べる層が食い違う。
pub(crate) fn planned_targets(options: &SwitchOptions) -> Result<&'static [SwitchTarget]> {
    Ok(switch_order(resolve_target(
        options.target,
        options.scope()?,
    )?))
}

/// 適用順に並んだ層の集合を、その集合をちょうど表す単一の target へ戻す。空集合は `None`。
///
/// [`switch_order`] の逆で、`update` が「まだ適用されていない層だけ」を [`run`] へ渡すために使う。
pub(crate) fn target_covering(targets: &[SwitchTarget]) -> Option<SwitchTarget> {
    match (
        targets.contains(&SwitchTarget::Home),
        targets.contains(&SwitchTarget::Darwin),
    ) {
        (true, true) => Some(SwitchTarget::All),
        (true, false) => Some(SwitchTarget::Home),
        (false, true) => Some(SwitchTarget::Darwin),
        (false, false) => None,
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

/// `home-manager switch --flake <config-dir>#<user>` の実行プログラムと引数を組み立てる。
/// 降格の要否は [`HomeApplyUser`] が決める。root で降格対象が無い状態はその型が構築を拒むため、
/// ここでは降格対象の有無だけを見る。
fn home_manager_invocation(
    config_dir: &Path,
    user: &str,
    downgrade_to: Option<&str>,
    home_manager: &OsString,
) -> SwitchInvocation {
    SwitchInvocation {
        target: SwitchTarget::Home,
        command: Invocation::downgraded(
            home_manager.clone(),
            [
                OsString::from("switch"),
                OsString::from("--flake"),
                flake_ref(config_dir, user),
            ],
            downgrade_to,
        ),
    }
}

/// `darwin-rebuild switch --flake <ref>` の実行プログラムと引数を、root 実行かどうかで決める。
///
/// 昇格の規則そのものは [`Invocation::escalated`] が持つ。euid は引数で受け取り副作用を持たない
/// （呼び出し側で euid を解決する）。
fn darwin_rebuild_invocation(
    darwin_rebuild: &OsString,
    flake_ref: OsString,
    is_root: bool,
) -> SwitchInvocation {
    SwitchInvocation {
        target: SwitchTarget::Darwin,
        command: Invocation::escalated(
            darwin_rebuild.clone(),
            [
                OsString::from("switch"),
                OsString::from("--flake"),
                flake_ref,
            ],
            is_root,
        ),
    }
}

/// `dotfiles switch` が実行する外部コマンド列を副作用なしで組み立てる。
fn switch_invocations(input: SwitchInvocationInput<'_>) -> Vec<SwitchInvocation> {
    input
        .targets
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
    /// 適用順に展開済みの層。`All` は含まない。
    targets: &'a [SwitchTarget],
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

/// 適用する層と、その層を適用する外部コマンドの起動。
///
/// 層を持つのは、Darwin 適用の前だけ `/etc` の退避が要るためである。
struct SwitchInvocation {
    target: SwitchTarget,
    command: Invocation,
}

/// nix-darwin が `/etc/static` リンクを作る前に、衝突する既存シェル起動ファイルだけを退避する。
fn prepare_nix_darwin_etc(dry_run: bool, deadline: Option<Instant>) -> Result<()> {
    if std::env::consts::OS != "macos" {
        return Ok(());
    }

    for path in [Path::new("/etc/bashrc"), Path::new("/etc/zshrc")] {
        move_etc_file_before_nix_darwin(path, dry_run, deadline)?;
    }

    Ok(())
}

/// 管理済みリンクは触らず、それ以外（通常ファイル・未管理シンボリックリンク）を
/// `<name>.before-nix-darwin` へ移動する。
fn move_etc_file_before_nix_darwin(
    path: &Path,
    dry_run: bool,
    deadline: Option<Instant>,
) -> Result<()> {
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
        deadline,
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

#[derive(Args, Clone)]
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

    /// root daemon の全ユーザー走査が、1 ユーザー分の適用を同じ実装で実行するための構築口。
    ///
    /// target は指定せず、対象ユーザーと設定ディレクトリだけを差し替える。適用範囲は
    /// [`resolve_target`] がそのユーザーの scope から決める。
    pub(crate) fn for_user(&self, user: &str, config_dir: PathBuf) -> Self {
        Self {
            target: None,
            user: Some(user.to_string()),
            config_dir: Some(config_dir),
            ..self.clone()
        }
    }

    /// 1 ユーザー分を指す指定が 1 つも無い root 実行だけを、このマシンの全ユーザー走査に倒す。
    ///
    /// auto-update daemon は `--host` だけを渡してこの形で起動する。`--host` はマシン単位の値なので
    /// 条件に含めない。target・`--user`・`--config-dir` は 1 ユーザー分の実行を指す指定であり、走査は
    /// [`Self::for_user`] でそれらを上書きする。1 つでも明示されていれば走査へ倒さず、明示された値を
    /// 捨てて要求より広い範囲を root 権限で適用しない。
    ///
    /// euid は引数で受け取る。判定を副作用なしにし、`--host` を含む argv の組み合わせを単体テストで
    /// 固定できるようにする。
    pub(crate) fn sweeps_all_users(&self, is_root: bool) -> bool {
        is_root && self.user.is_none() && self.target.is_none() && self.config_dir.is_none()
    }

    /// まだ適用されていない層だけを適用するために、target を絞った同じ options を作る。
    ///
    /// `update` が適用済みの層を外して [`run`] を呼ぶための構築口。target 以外の指定は変えない。
    pub(crate) fn narrowed_to(&self, target: SwitchTarget) -> Self {
        Self {
            target: Some(target),
            ..self.clone()
        }
    }

    /// `#<host>` として flake 属性名に使うホスト名。省略時はこのマシンの短いホスト名。
    pub(crate) fn host(&self) -> Result<String> {
        self.host.clone().map_or_else(current_host, Ok)
    }

    /// `update` が lock 更新と switch の両方を同じ予行実行モードで扱う。
    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }

    /// 適用後に Home Manager の世代を整理する処理が、適用と同じ実行ファイルを使うための入口。
    pub(crate) fn home_manager(&self) -> &OsString {
        &self.home_manager
    }

    /// 利用者所有ファイルを触る処理の対象ユーザーを解決する。
    ///
    /// `update` は lock 更新で config dir へ書くため、target に関わらずこれを先に解決する。
    pub(crate) fn home_apply_user(&self) -> Result<HomeApplyUser> {
        HomeApplyUser::resolve(self.user.clone(), is_effective_root())
    }

    /// 対象ユーザーがこのマシンで system 層まで持つか。対象ユーザーが決まらない実行では `false`。
    ///
    /// system 層を持つユーザーの home 層は、`home-manager switch` と nix-darwin の activation の
    /// どちらからも適用される。`update` はその 2 経路を知るためにこれを使う。
    pub(crate) fn owns_system_layer(&self) -> Result<bool> {
        Ok(self.scope()? == Some(ConfigScope::Full))
    }

    /// 対象ユーザーがこのマシンで持つ層の範囲。対象ユーザーが決まらない実行では `None`。
    fn scope(&self) -> Result<Option<ConfigScope>> {
        self.scope_user()?
            .map(|user| config_scope(&user))
            .transpose()
    }

    /// この実行で対象ユーザーが決まるならその名前、決まらないなら `None`。
    fn scope_user(&self) -> Result<Option<String>> {
        HomeApplyUser::scope_user(self.user.clone(), is_effective_root())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
/// `home` と `darwin` は独立して実行でき、`all` は Home Manager の後に Darwin を実行する。
pub(crate) enum SwitchTarget {
    Home,
    Darwin,
    All,
}

impl SwitchTarget {
    /// 端末へ層を示すときの名前。CLI が受け取る target 名と同じ綴りにする。
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Darwin => "darwin",
            Self::All => "all",
        }
    }
}

/// 適用対象を、明示指定と対象ユーザーの scope から決める。
///
/// 省略時はそのユーザーがこのマシンで持つ層をそのまま適用する。system 層を別ユーザーが持つ
/// マシンでは、Darwin を含む target を明示されても止める。生成 flake に darwin 出力が無いことに
/// よる nix の属性解決エラーへ委ねると、止まった理由が利用者に伝わらない。
///
/// scope が `None` になるのは対象ユーザーが決まらない実行（root で `--user` 省略）だけである。この
/// 場合は誰の層かを問えないので scope による絞り込みを行わず、省略時は `All` のままにする。Home を
/// 含む target は [`HomeApplyUser`] が対象ユーザー不在として止め、Darwin 単独 target はそのまま通す。
fn resolve_target(
    explicit: Option<SwitchTarget>,
    scope: Option<ConfigScope>,
) -> Result<SwitchTarget> {
    let target = explicit.unwrap_or(match scope {
        Some(ConfigScope::Home) => SwitchTarget::Home,
        Some(ConfigScope::Full) | None => SwitchTarget::All,
    });
    if scope == Some(ConfigScope::Home) && switch_order(target).contains(&SwitchTarget::Darwin) {
        bail!(
            "system 層はこのマシンの別のユーザーが管理しているため適用できない（target を省略すると home 層だけを適用する）"
        );
    }
    Ok(target)
}

/// `all` を Home Manager -> Darwin の適用順序へ展開する。
fn switch_order(target: SwitchTarget) -> &'static [SwitchTarget] {
    match target {
        SwitchTarget::Home => &[SwitchTarget::Home],
        SwitchTarget::Darwin => &[SwitchTarget::Darwin],
        SwitchTarget::All => &[SwitchTarget::Home, SwitchTarget::Darwin],
    }
}

/// 利用者所有ファイルを触る処理の対象ユーザー。
///
/// root 実行で `--user` を省略した状態はこの型を構築できない。省略を現在ユーザー（root）へ倒すと処理が root の
/// まま走り、利用者所有ファイル（Home Manager の生成物、`update` が書く `flake.lock`）の所有者が root へ変わる。
/// caller responsibility: 最初の特権コマンドより前に構築すること。後ろに置くと、拒む前に root で書き込みが済む。
#[derive(Debug)]
pub(crate) struct HomeApplyUser {
    name: String,
    /// root からこのユーザーへ降格して実行するか。
    downgrade_from_root: bool,
}

impl HomeApplyUser {
    /// 明示指定と実行時 euid から対象ユーザーを決める。root かつ未指定は `Err`。
    pub(crate) fn resolve(explicit: Option<String>, is_root: bool) -> Result<Self> {
        let Some(name) = Self::scope_user(explicit, is_root)? else {
            bail!(
                "root で利用者所有ファイルを書くには `--user` が必要（省略すると Home Manager の生成物や `flake.lock` が root 所有になる）"
            )
        };
        Ok(Self {
            name,
            downgrade_from_root: is_root,
        })
    }

    /// 利用者所有ファイルを書かない処理が、同じ入力から対象ユーザー名だけを得るための入口。
    ///
    /// root で `--user` を省略した状態は誰の flake を扱うかが決まらない状態であり、[`Self::resolve`]
    /// が構築を拒む状態と同じである。名前が要るだけの呼び出し側はここで `None` を受け取り、対象
    /// ユーザーに紐づく判断（scope による適用範囲の絞り込み）を行わない。
    fn scope_user(explicit: Option<String>, is_root: bool) -> Result<Option<String>> {
        match (is_root, explicit) {
            (true, None) => Ok(None),
            (_, Some(name)) => Ok(Some(name)),
            (false, None) => current_user().map(Some),
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
pub(crate) fn is_effective_root() -> bool {
    rustix::process::geteuid().is_root()
}

/// `darwin_rebuild_invocation` が euid に応じて sudo 前置の有無を切り替えること、適用範囲が対象
/// ユーザーの scope から決まること、および全ユーザー走査へ倒す条件を検証する。
#[cfg(test)]
mod tests {
    use super::{
        ConfigScope, HomeApplyUser, SwitchInvocationInput, SwitchOptions, SwitchTarget,
        darwin_rebuild_invocation, resolve_target, switch_invocations, switch_order,
        target_covering,
    };
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    /// 走査条件に関わる指定だけを差し替えた `SwitchOptions` を作る。
    ///
    /// 外部コマンド名と予行実行は走査条件に入らないので既定値で固定する。
    fn options(
        target: Option<SwitchTarget>,
        user: Option<&str>,
        host: Option<&str>,
        config_dir: Option<&str>,
    ) -> SwitchOptions {
        SwitchOptions {
            target,
            user: user.map(str::to_string),
            host: host.map(str::to_string),
            config_dir: config_dir.map(PathBuf::from),
            home_manager: OsString::from("home-manager"),
            darwin_rebuild: OsString::from("darwin-rebuild"),
            dry_run: false,
        }
    }

    /// root 実行では sudo を前置せず `darwin-rebuild switch` を直接起動する。
    #[test]
    fn root_invocation_runs_darwin_rebuild_without_sudo() {
        let invocation = darwin_rebuild_invocation(
            &OsString::from("darwin-rebuild"),
            OsString::from("/cfg#host"),
            true,
        );

        assert_eq!(invocation.command.program, OsString::from("darwin-rebuild"));
        assert_eq!(
            invocation.command.args,
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

        assert_eq!(invocation.command.program, OsString::from("sudo"));
        assert_eq!(
            invocation.command.args,
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

    /// system 層を自分が持つマシンでは、target 省略で Home Manager と nix-darwin の両方を適用する。
    #[test]
    fn full_scope_defaults_to_all() -> anyhow::Result<()> {
        assert_eq!(
            resolve_target(None, Some(ConfigScope::Full))?,
            SwitchTarget::All
        );
        Ok(())
    }

    /// system 層を別ユーザーが持つマシンでは、target 省略で Home Manager だけを適用する。
    #[test]
    fn home_scope_defaults_to_home() -> anyhow::Result<()> {
        assert_eq!(
            resolve_target(None, Some(ConfigScope::Home))?,
            SwitchTarget::Home
        );
        Ok(())
    }

    /// home scope で Darwin を含む target を明示しても、理由を示して止める。
    #[test]
    fn home_scope_refuses_targets_that_include_darwin() {
        for target in [SwitchTarget::Darwin, SwitchTarget::All] {
            let err = resolve_target(Some(target), Some(ConfigScope::Home))
                .err()
                .map(|err| err.to_string())
                .unwrap_or_default();
            assert!(err.contains("system 層"), "{err}");
        }
    }

    /// 対象ユーザーが決まらない実行では scope で絞り込まない。Darwin 単独 target は利用者所有
    /// ファイルを書かないので、`sudo dotfiles switch darwin` をここで止めない。
    #[test]
    fn unknown_scope_keeps_explicit_darwin() -> anyhow::Result<()> {
        assert_eq!(
            resolve_target(Some(SwitchTarget::Darwin), None)?,
            SwitchTarget::Darwin
        );
        Ok(())
    }

    /// 対象ユーザーが決まらない実行の target 省略時は `All` のままにし、Home を含めることで
    /// `HomeApplyUser` 側に対象ユーザー不在として止めさせる。
    #[test]
    fn unknown_scope_defaults_to_all() -> anyhow::Result<()> {
        assert_eq!(resolve_target(None, None)?, SwitchTarget::All);
        Ok(())
    }

    /// root 実行で 1 ユーザー分を指す指定が 1 つも無いときだけ、全ユーザー走査へ倒す。
    ///
    /// `--host` はマシン単位の値なので走査条件に入らない。auto-update daemon が渡す argv 形が
    /// `dotfiles update --host <host>` であり、これを 1 ユーザー分の指定として扱うと daemon が
    /// 所有者 1 人しか更新しなくなる。
    #[test]
    fn root_without_narrowing_options_sweeps_all_users() {
        assert!(options(None, None, None, None).sweeps_all_users(true));
        assert!(options(None, None, Some("mac"), None).sweeps_all_users(true));
    }

    /// 1 ユーザー分を指す指定が 1 つでもあれば走査へ倒さない。明示された値を捨てて要求より広い
    /// 範囲を root 権限で適用しない。`--host` の有無は結果を変えない。
    #[test]
    fn root_with_narrowing_option_does_not_sweep() {
        for host in [None, Some("mac")] {
            for narrowed in [
                options(Some(SwitchTarget::Darwin), None, host, None),
                options(Some(SwitchTarget::Home), None, host, None),
                options(Some(SwitchTarget::All), None, host, None),
                options(None, Some("alice"), host, None),
                options(None, None, host, Some("/cfg")),
            ] {
                assert!(!narrowed.sweeps_all_users(true));
            }
        }
    }

    /// 非 root 実行は指定の有無にかかわらず自分だけを対象にする。
    #[test]
    fn non_root_never_sweeps_all_users() {
        for narrowed in [
            options(None, None, None, None),
            options(None, None, Some("mac"), None),
            options(
                Some(SwitchTarget::All),
                Some("alice"),
                Some("mac"),
                Some("/cfg"),
            ),
        ] {
            assert!(!narrowed.sweeps_all_users(false));
        }
    }

    /// `all` は standalone Home Manager を先に適用してから nix-darwin を適用する。
    #[test]
    fn all_expands_to_home_manager_then_darwin() {
        assert_eq!(
            switch_order(SwitchTarget::All),
            &[SwitchTarget::Home, SwitchTarget::Darwin]
        );
    }

    /// 適用する層の部分集合は、その集合をちょうど表す target へ戻る。`update` は適用済みの層を外した
    /// 残りをこの形で `run` へ渡すため、往復で層が増減してはならない。
    #[test]
    fn target_covering_is_the_inverse_of_switch_order() {
        for target in [SwitchTarget::Home, SwitchTarget::Darwin, SwitchTarget::All] {
            assert_eq!(target_covering(switch_order(target)), Some(target));
        }
    }

    /// 全層が適用済みなら渡す層が無い。`update` はこれを適用も掃除もしない合図に使う。
    #[test]
    fn target_covering_of_nothing_is_none() {
        assert_eq!(target_covering(&[]), None);
    }

    /// `all` 経路が Home Manager を適用してから nix-darwin を適用するコマンド列を組み立てる。
    #[test]
    fn all_invocations_run_home_manager_then_darwin() {
        let home_manager = OsString::from("home-manager");
        let darwin_rebuild = OsString::from("darwin-rebuild");
        let invocations = switch_invocations(SwitchInvocationInput {
            targets: switch_order(SwitchTarget::All),
            config_dir: Path::new("/cfg"),
            user: "alice",
            host: "mac",
            downgrade_to: None,
            home_manager: &home_manager,
            darwin_rebuild: &darwin_rebuild,
            is_root: false,
        });

        assert_eq!(invocations.len(), 2);
        assert_eq!(
            invocations[0].command.program,
            OsString::from("home-manager")
        );
        assert_eq!(
            invocations[0].command.args,
            vec![
                OsString::from("switch"),
                OsString::from("--flake"),
                OsString::from("/cfg#alice"),
            ]
        );
        assert_eq!(invocations[1].command.program, OsString::from("sudo"));
        assert_eq!(
            invocations[1].command.args,
            vec![
                OsString::from("darwin-rebuild"),
                OsString::from("switch"),
                OsString::from("--flake"),
                OsString::from("/cfg#mac"),
            ]
        );
    }
}
