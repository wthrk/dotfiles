//! `dotfiles update` がローカル flake の lock を更新してから適用する処理。
//!
//! `switch` は lock 済みの入力をそのまま使う。main など更新される参照へ追従したいときだけ、
//! このコマンドで `$HOME/.config/dotfiles/flake.lock` を先に更新してから既存の適用処理を実行する。
//!
//! root 実行で 1 ユーザー分を指す指定が 1 つも無いときは、このマシンでローカル flake を持つ
//! 全ユーザーを更新する。auto-update daemon はこの形で起動し、更新の仕組みをユーザーの種類で
//! 分けない。

use std::{ffi::OsString, path::Path};

use anyhow::bail;
use clap::Args;

use crate::{
    Result,
    environment::local_flake_accounts,
    local_flake::INPUT_NAME,
    process::{run as run_process, sudo_as_user_args},
    switch,
};

const DEFAULT_NIX_PROGRAM: &str = "/nix/var/nix/profiles/default/bin/nix";

/// 既存の `switch` と同じオプションを受け取り、先に flake.lock を更新する。
pub(crate) fn run(options: UpdateOptions) -> Result<()> {
    if options.switch.sweeps_all_users()? {
        return run_all_users(&options.switch);
    }
    run_one_user(options.switch)
}

/// 1 ユーザー分の lock 更新と適用を、既存の `switch` と同じ経路で実行する。
fn run_one_user(options: switch::SwitchOptions) -> Result<()> {
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
    )?;
    switch::run(options)
}

/// auto-update daemon の経路。ローカル flake を持つ全ユーザーを、そのユーザー権限で更新する。
///
/// lock 更新と Home Manager は `HomeApplyUser` の降格経路で対象ユーザーへ落とすため、root のまま
/// 他ユーザーの flake を評価・ビルドしない。system 層はそのユーザーの scope が `Full` のとき、
/// すなわち `/etc/profiles/per-user/` が所有者として示すユーザーのときだけ適用される。
///
/// 1 ユーザーの失敗で走査を打ち切らない。走査順はユーザー名の昇順なので、途中で伝播させると
/// 名前が先に来るユーザーの flake が壊れているだけで、後続ユーザーの更新も所有者の system 層適用も
/// 実行されなくなる。失敗は記録して全ユーザーを試行し、1 件でもあれば最後に非 0 で終了する。
fn run_all_users(base: &switch::SwitchOptions) -> Result<()> {
    let mut failed = Vec::new();
    for account in local_flake_accounts()? {
        println!("==> dotfiles update: {}", account.user);
        if let Err(error) = run_one_user(base.for_user(&account.user, account.config_dir)) {
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
fn update_lock(config_dir: &Path, dry_run: bool, lock_owner: Option<&str>) -> Result<()> {
    let invocation = update_lock_invocation(config_dir, lock_owner);
    run_process(invocation.program, invocation.args, dry_run)
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
fn update_lock_invocation(config_dir: &Path, lock_owner: Option<&str>) -> UpdateLockInvocation {
    let args = update_lock_args(config_dir);
    let nix = OsString::from(DEFAULT_NIX_PROGRAM);
    if let Some(user) = lock_owner {
        UpdateLockInvocation {
            program: OsString::from("sudo"),
            args: sudo_as_user_args(user, nix, args),
        }
    } else {
        UpdateLockInvocation { program: nix, args }
    }
}

/// `nix flake update` 実行の起動プログラムと引数列。
struct UpdateLockInvocation {
    program: OsString,
    args: Vec<OsString>,
}

#[derive(Args)]
/// ローカル flake の入力を更新してから、既存の switch と同じ対象を適用する。
pub(crate) struct UpdateOptions {
    #[command(flatten)]
    switch: switch::SwitchOptions,
}

/// `update_lock_args` が `dotfiles` input だけを対象に `nix flake update` を組むこと、および
/// 全ユーザー走査の終了状態が失敗したユーザー全件から決まることを検証する。
#[cfg(test)]
mod tests {
    use super::{sweep_result, update_lock_args, update_lock_invocation};
    use std::ffi::OsString;
    use std::path::Path;

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
}
