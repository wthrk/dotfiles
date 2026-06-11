//! `dotfiles update` がローカル flake の lock を更新してから適用する処理。
//!
//! `switch` は lock 済みの入力をそのまま使う。main など更新される参照へ追従したいときだけ、
//! このコマンドで `$HOME/.config/dotfiles/flake.lock` を先に更新してから既存の適用処理を実行する。

use std::{ffi::OsString, path::Path};

use clap::Args;

use crate::{
    Result,
    local_flake::INPUT_NAME,
    process::{run as run_process, sudo_as_user_args},
    switch,
};

/// 既存の `switch` と同じオプションを受け取り、先に flake.lock を更新する。
pub(crate) fn run(options: UpdateOptions) -> Result<()> {
    let config_dir = options.switch.config_dir()?;
    switch::ensure_config_exists(&config_dir)?;
    let lock_owner = options.switch.root_user_override().map(str::to_owned);
    update_lock(&config_dir, options.switch.dry_run(), lock_owner.as_deref())?;
    switch::run(options.switch)
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
    if let Some(user) = lock_owner {
        UpdateLockInvocation {
            program: OsString::from("sudo"),
            args: sudo_as_user_args(user, OsString::from("nix"), args),
        }
    } else {
        UpdateLockInvocation {
            program: OsString::from("nix"),
            args,
        }
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

/// `update_lock_args` が `dotfiles` input だけを対象に `nix flake update` を組むことを検証する。
#[cfg(test)]
mod tests {
    use super::{update_lock_args, update_lock_invocation};
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

    /// root daemon の `--user` 経路では lock 更新も対象ユーザーの HOME/uid で実行する。
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
                OsString::from("nix"),
                OsString::from("flake"),
                OsString::from("update"),
                OsString::from("dotfiles"),
                OsString::from("--flake"),
                OsString::from("/cfg"),
            ]
        );
    }
}
