//! `dotfiles update` がローカル flake の lock を更新してから適用する処理。
//!
//! `switch` は lock 済みの入力をそのまま使う。main など更新される参照へ追従したいときだけ、
//! このコマンドで `$HOME/.config/dotfiles/flake.lock` を先に更新してから既存の適用処理を実行する。
//!
//! root 実行で 1 ユーザー分を指す指定が 1 つも無いときは、このマシンでローカル flake を持つ
//! 全ユーザーを更新する。auto-update daemon はこの形で起動し、更新の仕組みをユーザーの種類で
//! 分けない。
//!
//! lock が指す pin を既に適用し終えている層は、適用も掃除もしない。適用済みの層へ
//! `home-manager switch` と `darwin-rebuild switch` を起動すると、Homebrew の無人 upgrade を含む
//! activation 一式と store の掃除を毎回やり直すことになる。判定は [`is_applied`] が層ごとに行い、
//! 手動と無人の両経路が通る [`run_one_user`] に置く。経路ごとに違う判定を持たせない。
//!
//! 適用が作った世代は GC root なので、掃除しない限り旧世代とその閉包が store に残り続ける。適用の
//! 直後に、その実行が適用した層の世代だけを [`GENERATION_RETENTION_DAYS`] で切って掃除する。

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::bail;
use clap::Args;

use crate::{
    Result,
    environment::{local_flake_accounts, user_home},
    local_flake::{INPUT_NAME, escape_nix_string},
    process::Invocation,
    switch::{self, SwitchTarget},
};

const DEFAULT_NIX_PROGRAM: &str = "/nix/var/nix/profiles/default/bin/nix";
/// store の掃除を起動する絶対パス。`DEFAULT_NIX_PROGRAM` と同じく PATH に依存させない。
const DEFAULT_NIX_COLLECT_GARBAGE_PROGRAM: &str =
    "/nix/var/nix/profiles/default/bin/nix-collect-garbage";

/// home 層の適用完了を示す、Home Manager がホーム配下に置く GC root。
///
/// Home Manager は activation の全ステップを終えた後にこのリンクを張り替え、次回の activation では
/// これを「いま適用されている世代」として読む。ビルドと profile の切り替えより後に書かれるため、
/// ビルドだけ済んで activation が失敗した状態はこのリンクに現れない。
const HOME_GENERATION_LINK: &str = ".local/state/home-manager/gcroots/current-home";

/// system 層の適用完了を示す、nix-darwin が張り替えるリンク。
///
/// nix-darwin の `activate` はこのリンクを最後に張り替える。profile（`/nix/var/nix/profiles/system`）は
/// ビルド直後に切り替わるので、activation が途中で止まった世代も profile には現れる。適用完了の判定に
/// 使えるのはこちらだけである。
const SYSTEM_GENERATION_LINK: &str = "/run/current-system";

/// 掃除の対象から外す世代の年齢。この日数より新しい世代は残す。
///
/// `README.md` の「ロールバック」が案内する `darwin-rebuild switch --rollback` と
/// `home-manager switch --rollback` は残った世代へ戻るため、この日数がロールバックできる範囲になる。
const GENERATION_RETENTION_DAYS: u32 = 30;

/// 全ユーザー走査で 1 ユーザー分の処理に与える上限。
///
/// この経路が起動する外部コマンドは nightly CI の `bump` job と同じ nix 作業（`nix flake update` と
/// 構成の評価・ビルド）であり、その job は store を持たない macOS runner に対して
/// `.github/workflows/nightly-update.yml` の `timeout-minutes: 120` で足りている。auto-update daemon は
/// store が温まった実機で走るので、同じ 120 分は正常な更新を打ち切らない上限として十分に広い。
/// daemon の発火間隔（`nix/darwin.nix` の `StartCalendarInterval`）より長いが、launchd は同じ label の
/// job が走っている間の発火を落とすため、走査が次の発火に重なっても二重には起動しない。
const USER_UPDATE_TIMEOUT: Duration = Duration::from_secs(120 * 60);

/// 既存の `switch` と同じオプションを受け取り、先に flake.lock を更新する。
pub(crate) fn run(options: UpdateOptions) -> Result<()> {
    if options.switch.sweeps_all_users(switch::is_effective_root()) {
        return run_all_users(&options.switch);
    }
    // 利用者自身の実行は中断できるので期限を置かない。
    run_one_user(options.switch, None)
}

/// 1 ユーザー分の lock 更新と適用を、既存の `switch` と同じ経路で実行し、適用が増やした旧世代を掃除する。
///
/// lock 更新の後に、適用する層それぞれが lock の指す pin で適用済みかを [`is_applied`] で調べ、まだ
/// 適用されていない層だけを `switch` へ渡す。全層が適用済みなら適用も掃除も起動しない。手動実行も
/// この関数を通るため、判定は無人経路だけの分岐にならない。
///
/// `deadline` を渡すと、このユーザー分の外部コマンドはその時刻で打ち切られる。
fn run_one_user(options: switch::SwitchOptions, deadline: Option<Instant>) -> Result<()> {
    let config_dir = options.config_dir()?;
    switch::ensure_config_exists(&config_dir)?;
    // 降格対象は最初の特権コマンドより前に解決する。lock 更新は config dir へ書くため、root 実行で
    // 降格対象が無いまま進むと利用者所有の `flake.lock` が root 所有へ変わる。`HomeApplyUser` は
    // その組み合わせで構築を拒むので、ここで解決しておけば argv を組み立てる前に落ちる。
    let target_user = options.home_apply_user()?;
    update_lock(
        &config_dir,
        options.dry_run(),
        target_user.downgrade_target(),
        deadline,
    )?;
    let Some(target) = pending_target(&options, &target_user, &config_dir, deadline)? else {
        return Ok(());
    };
    let applied = switch::run(options.narrowed_to(target), deadline)?;
    collect_garbage(
        &options,
        applied,
        target_user.downgrade_target(),
        switch::is_effective_root(),
        deadline,
    )
}

/// この実行で適用する層のうち、まだ適用されていないものだけを表す target を返す。全層が適用済みなら
/// `None`。
///
/// 適用済みの層は端末へ 1 行ずつ示す。auto-update daemon のログに残るのはこの出力なので、何も起動
/// しなかった実行が「何も書かずに終わった実行」と見分けられなくならないようにする。
///
/// 予行実行は外部コマンドを起動しないので判定も行わず、適用する層をそのまま返す。`--dry-run` は
/// 「この実行が何を起動するか」を示すためのもので、判定のために `nix eval` を起動しては予行でなくなる。
fn pending_target(
    options: &switch::SwitchOptions,
    target_user: &switch::HomeApplyUser,
    config_dir: &Path,
    deadline: Option<Instant>,
) -> Result<Option<SwitchTarget>> {
    let planned = switch::planned_targets(options)?;
    if options.dry_run() {
        return Ok(switch::target_covering(planned));
    }
    let pending = planned
        .iter()
        .copied()
        .filter(|target| {
            let applied = is_applied(*target, options, target_user, config_dir, deadline);
            if applied {
                println!("==> {} 層は適用済みのため飛ばします", target.label());
            }
            !applied
        })
        .collect::<Vec<_>>();
    Ok(switch::target_covering(&pending))
}

/// 対象の層が、いま lock が指す pin で適用済みかを判定する。
///
/// 判定材料は、その層の適用が完了したときにだけ書き換わるリンク（[`HOME_GENERATION_LINK`]、
/// [`SYSTEM_GENERATION_LINK`]）と、同じ pin から評価した store path の一致である。どちらも
/// Home Manager と nix-darwin が自分で作る成果物で、適用状態を写した marker を別に持たない。
///
/// 判定できない事象——評価の失敗、リンクが読めない、ホーム／ホスト名が解決できない——はすべて未適用へ
/// 倒す。適用漏れを見逃すより余分に適用するほうが安全であり、同じ失敗は続く適用経路がそのまま報告する。
fn is_applied(
    target: SwitchTarget,
    options: &switch::SwitchOptions,
    target_user: &switch::HomeApplyUser,
    config_dir: &Path,
    deadline: Option<Instant>,
) -> bool {
    let (attribute, activated) = match target {
        SwitchTarget::Home => (
            home_attribute(target_user.name()),
            user_home(target_user.name()).map(|home| home.join(HOME_GENERATION_LINK)),
        ),
        SwitchTarget::Darwin => match options.host() {
            Ok(host) => (
                darwin_attribute(&host),
                Some(PathBuf::from(SYSTEM_GENERATION_LINK)),
            ),
            Err(_) => return false,
        },
        SwitchTarget::All => unreachable!("SwitchTarget::All is expanded before execution"),
    };
    let Some(activated) = activated.and_then(|link| fs::read_link(link).ok()) else {
        return false;
    };
    evaluate_store_path(
        config_dir,
        &attribute,
        target_user.downgrade_target(),
        deadline,
    )
    .is_some_and(|evaluated| evaluated == activated)
}

/// 生成 flake の属性を評価して store path を求める。求まらなければ `None`。
///
/// `nix eval` は評価だけを行い、評価結果の store path をビルドしない。適用済みかを調べるために、
/// まだビルドしていない構成を実機へ持ち込むことはない。lock はこの直前に更新済みなので、評価が
/// lock を書き換えないよう `--no-update-lock-file` を渡す。
fn evaluate_store_path(
    config_dir: &Path,
    attribute: &str,
    downgrade_to: Option<&str>,
    deadline: Option<Instant>,
) -> Option<PathBuf> {
    let evaluated = Invocation::downgraded(
        OsString::from(DEFAULT_NIX_PROGRAM),
        evaluate_store_path_args(config_dir, attribute),
        downgrade_to,
    )
    .run_capture(deadline)
    .ok()?;
    let evaluated = evaluated.trim();
    (!evaluated.is_empty()).then(|| PathBuf::from(evaluated))
}

/// `nix eval --no-update-lock-file --raw <config-dir>#<属性>` の引数列を組み立てる純粋関数。
fn evaluate_store_path_args(config_dir: &Path, attribute: &str) -> Vec<OsString> {
    let mut reference = config_dir.as_os_str().to_os_string();
    reference.push("#");
    reference.push(attribute);
    [
        OsString::from("eval"),
        OsString::from("--no-update-lock-file"),
        OsString::from("--raw"),
        reference,
    ]
    .into_iter()
    .collect()
}

/// home 層の適用対象を指す属性パス。`home-manager switch --flake <dir>#<user>` と同じ構成を指す。
///
/// 名前は生成 flake が `homeConfigurations` のキーを書くのと同じ [`escape_nix_string`] で引用する。
fn home_attribute(user: &str) -> String {
    format!(
        r#"homeConfigurations."{}".activationPackage.outPath"#,
        escape_nix_string(user)
    )
}

/// system 層の適用対象を指す属性パス。`darwin-rebuild switch --flake <dir>#<host>` と同じ構成を指す。
/// 名前の引用は [`home_attribute`] と同じ。
fn darwin_attribute(host: &str) -> String {
    format!(
        r#"darwinConfigurations."{}".system.outPath"#,
        escape_nix_string(host)
    )
}

/// 適用した層に対応する旧世代の掃除を、適用の直後に実行する。
///
/// 掃除の失敗はこのユーザーの更新の失敗として返る。全ユーザー走査では [`run_all_users`] が 1 ユーザー分の
/// 失敗として記録し、次のユーザーへ進む。
fn collect_garbage(
    options: &switch::SwitchOptions,
    applied: &[switch::SwitchTarget],
    downgrade_to: Option<&str>,
    is_root: bool,
    deadline: Option<Instant>,
) -> Result<()> {
    for invocation in
        garbage_collection_invocations(applied, options.home_manager(), downgrade_to, is_root)
    {
        invocation.run(options.dry_run(), deadline)?;
    }
    Ok(())
}

/// 適用した層に対応する掃除のコマンド列を、副作用なしで組み立てる。
///
/// home 層を適用したなら対象ユーザーの Home Manager 世代を、system 層を適用したならマシンの store を
/// 掃除する。層で分けるのは必要な権限が違うためで、Home Manager の世代整理は symlink を消すだけなので
/// 対象ユーザーの権限で足り、store の掃除は root の権限を要する。system 層を適用しない実行は store を
/// 触らず、そのマシンの store は所有者の更新が掃除する。
///
/// euid と降格対象は引数で受け取り、判定は呼び出し側で解決する。
fn garbage_collection_invocations(
    applied: &[switch::SwitchTarget],
    home_manager: &OsString,
    downgrade_to: Option<&str>,
    is_root: bool,
) -> Vec<Invocation> {
    [
        applied.contains(&switch::SwitchTarget::Home).then(|| {
            Invocation::downgraded(
                home_manager.clone(),
                expire_home_generations_args(),
                downgrade_to,
            )
        }),
        applied.contains(&switch::SwitchTarget::Darwin).then(|| {
            Invocation::escalated(
                OsString::from(DEFAULT_NIX_COLLECT_GARBAGE_PROGRAM),
                collect_store_args(),
                is_root,
            )
        }),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// `home-manager expire-generations -<日数> days` の引数列を組み立てる純粋関数。
///
/// Home Manager の世代は利用者の nix state ディレクトリにあり、root の `nix-collect-garbage` が探す
/// profile には入らない。現世代は Home Manager 自身が対象から外す。
fn expire_home_generations_args() -> Vec<OsString> {
    [
        OsString::from("expire-generations"),
        OsString::from(format!("-{GENERATION_RETENTION_DAYS} days")),
    ]
    .into_iter()
    .collect()
}

/// `nix-collect-garbage --delete-older-than <日数>d` の引数列を組み立てる純粋関数。
fn collect_store_args() -> Vec<OsString> {
    [
        OsString::from("--delete-older-than"),
        OsString::from(format!("{GENERATION_RETENTION_DAYS}d")),
    ]
    .into_iter()
    .collect()
}

/// auto-update daemon の経路。ローカル flake を持つ全ユーザーを、そのユーザー権限で更新する。
///
/// lock 更新と Home Manager は `HomeApplyUser` の降格経路で対象ユーザーへ落とすため、root のまま
/// 他ユーザーの flake を評価・ビルドしない。system 層はそのユーザーの scope が `Full` のとき、
/// すなわち `/etc/profiles/per-user/` が所有者として示すユーザーのときだけ適用される。
///
/// 走査はユーザー名の昇順で、1 ユーザーの失敗では止めない。失敗は記録して次のユーザーへ進み、
/// 1 件でもあれば最後に非 0 で終了する。1 ユーザーに与える時間は [`USER_UPDATE_TIMEOUT`] までで、
/// 超過も失敗として記録する。
fn run_all_users(base: &switch::SwitchOptions) -> Result<()> {
    let mut failed = Vec::new();
    for account in local_flake_accounts()? {
        println!("==> dotfiles update: {}", account.user);
        let result = run_one_user(
            base.for_user(&account.user, account.config_dir),
            Some(Instant::now() + USER_UPDATE_TIMEOUT),
        );
        if let Err(error) = result {
            eprintln!("==> dotfiles update failed: {}: {error:#}", account.user);
            failed.push(account.user);
        }
    }
    sweep_result(&failed)
}

/// 全ユーザー走査の終了状態を、失敗したユーザー名から決める。
///
/// daemon の `StandardErrorPath` に残るのはこのメッセージなので、最初の 1 件ではなく失敗した
/// ユーザーをすべて並べる。
fn sweep_result(failed: &[String]) -> Result<()> {
    if failed.is_empty() {
        Ok(())
    } else {
        bail!("dotfiles update failed for: {}", failed.join(", "))
    }
}

/// 生成済みローカル flake の `dotfiles` input だけを再 lock する。
///
/// 全 input を更新すると各端末が CI bump 済みの repo lock ではなく独自に最新 nixpkgs/taps へ進み、
/// fleet pin から乖離する。`dotfiles` input のみを更新し、推移的 nixpkgs/taps を repo の committed
/// lock に追従させる。
fn update_lock(
    config_dir: &Path,
    dry_run: bool,
    lock_owner: Option<&str>,
    deadline: Option<Instant>,
) -> Result<()> {
    update_lock_invocation(config_dir, lock_owner).run(dry_run, deadline)
}

/// `nix flake update <dotfiles> --flake <config-dir>` の引数列を組み立てる純粋関数。
fn update_lock_args(config_dir: &Path) -> Vec<OsString> {
    [
        OsString::from("flake"),
        OsString::from("update"),
        OsString::from(INPUT_NAME),
        OsString::from("--flake"),
        config_dir.as_os_str().to_os_string(),
    ]
    .into_iter()
    .collect()
}

/// lock 更新を root のまま行うか、対象ユーザーへ降格して行うかを引数列へ反映する。
fn update_lock_invocation(config_dir: &Path, lock_owner: Option<&str>) -> Invocation {
    Invocation::downgraded(
        OsString::from(DEFAULT_NIX_PROGRAM),
        update_lock_args(config_dir),
        lock_owner,
    )
}

#[derive(Args)]
/// ローカル flake の入力を更新してから、既存の switch と同じ対象を適用する。
pub(crate) struct UpdateOptions {
    #[command(flatten)]
    switch: switch::SwitchOptions,
}

/// `update_lock_args` が `dotfiles` input だけを対象に `nix flake update` を組むこと、適用済み判定が
/// 層ごとに正しい属性を評価すること、全ユーザー走査の終了状態が失敗したユーザー全件から決まること、
/// および store の掃除が euid で昇格を切り替えることを検証する。
#[cfg(test)]
mod tests {
    use super::{
        collect_store_args, darwin_attribute, evaluate_store_path_args,
        garbage_collection_invocations, home_attribute, sweep_result, switch, update_lock_args,
        update_lock_invocation,
    };
    use std::ffi::OsString;
    use std::path::Path;

    /// 適用済み判定は、`home-manager switch --flake <dir>#<user>` が適用するのと同じ構成を評価する。
    #[test]
    fn home_attribute_points_at_the_activation_package() {
        assert_eq!(
            home_attribute("ya-n"),
            r#"homeConfigurations."ya-n".activationPackage.outPath"#
        );
    }

    /// 適用済み判定は、`darwin-rebuild switch --flake <dir>#<host>` が適用するのと同じ構成を評価する。
    /// 名前は引用符で囲み、`macbook.air` のようにドットを含むホスト名でも 1 つの属性にする。
    #[test]
    fn darwin_attribute_points_at_the_system() {
        assert_eq!(
            darwin_attribute("macbook.air"),
            r#"darwinConfigurations."macbook.air".system.outPath"#
        );
    }

    /// 評価はビルドを起こさず、直前に更新した lock も書き換えない。
    #[test]
    fn evaluation_neither_builds_nor_writes_the_lock() {
        assert_eq!(
            evaluate_store_path_args(Path::new("/cfg"), "attr"),
            vec![
                OsString::from("eval"),
                OsString::from("--no-update-lock-file"),
                OsString::from("--raw"),
                OsString::from("/cfg#attr"),
            ]
        );
    }

    /// 全 input 更新ではなく `dotfiles` input 名付きで repo pin に追従させる。
    #[test]
    fn update_lock_args_targets_dotfiles_input() {
        let args = update_lock_args(Path::new("/cfg"));

        assert_eq!(
            args,
            vec![
                OsString::from("flake"),
                OsString::from("update"),
                OsString::from("dotfiles"),
                OsString::from("--flake"),
                OsString::from("/cfg"),
            ]
        );
    }

    /// root daemon の `--user` 経路では lock 更新も対象ユーザーの HOME/uid で実行し、`nix` は絶対パスで起動する。
    #[test]
    fn update_lock_with_owner_runs_nix_as_target_user() {
        let invocation = update_lock_invocation(Path::new("/cfg"), Some("alice"));

        assert_eq!(invocation.program, OsString::from("sudo"));
        assert_eq!(
            invocation.args,
            vec![
                OsString::from("-H"),
                OsString::from("-u"),
                OsString::from("alice"),
                OsString::from("env"),
                OsString::from(format!(
                    "PATH={}",
                    std::env::var("PATH").unwrap_or_default()
                )),
                OsString::from("/nix/var/nix/profiles/default/bin/nix"),
                OsString::from("flake"),
                OsString::from("update"),
                OsString::from("dotfiles"),
                OsString::from("--flake"),
                OsString::from("/cfg"),
            ]
        );
    }

    /// 全ユーザーが成功した走査は成功として終わる。
    #[test]
    fn sweep_without_failure_succeeds() -> anyhow::Result<()> {
        sweep_result(&[])
    }

    /// 途中のユーザーが失敗した走査は非 0 で終わり、失敗したユーザーを全件示す。
    #[test]
    fn sweep_with_failures_reports_every_failed_user() {
        let err = sweep_result(&["dotfilesci".to_string(), "ya".to_string()])
            .err()
            .map(|err| err.to_string())
            .unwrap_or_default();
        assert!(err.contains("dotfilesci"), "{err}");
        assert!(err.contains("ya"), "{err}");
    }

    #[test]
    fn update_lock_without_owner_uses_absolute_nix_path() {
        let invocation = update_lock_invocation(Path::new("/cfg"), None);
        assert_eq!(
            invocation.program,
            OsString::from("/nix/var/nix/profiles/default/bin/nix")
        );
    }

    /// 利用者自身の実行では store の掃除を `sudo` で昇格し、root daemon では直接起動する。
    #[test]
    fn store_collection_escalates_only_when_not_root() {
        let home_manager = OsString::from("home-manager");
        let program = OsString::from("/nix/var/nix/profiles/default/bin/nix-collect-garbage");
        let applied = [switch::SwitchTarget::Darwin];

        let as_root = garbage_collection_invocations(&applied, &home_manager, None, true);
        assert_eq!(as_root[0].program, program);
        assert_eq!(as_root[0].args, collect_store_args());

        let as_user = garbage_collection_invocations(&applied, &home_manager, None, false);
        assert_eq!(as_user[0].program, OsString::from("sudo"));
        assert_eq!(as_user[0].args[0], program);
        assert_eq!(as_user[0].args[1..], collect_store_args()[..]);
    }
}
