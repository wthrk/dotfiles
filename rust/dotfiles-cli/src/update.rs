//! `dotfiles update` がローカル flake の lock を更新してから適用する処理。
//!
//! `switch` は lock 済みの入力をそのまま使う。main など更新される参照へ追従したいときだけ、
//! このコマンドで `$HOME/.config/dotfiles/flake.lock` を先に更新してから既存の適用処理を実行する。

use std::{ffi::OsString, path::Path};

use clap::Args;

use crate::{Result, process::run as run_process, switch};

/// 既存の `switch` と同じオプションを受け取り、先に flake.lock を更新する。
pub(crate) fn run(options: UpdateOptions) -> Result<()> {
    let config_dir = options.switch.config_dir()?;
    switch::ensure_config_exists(&config_dir)?;
    update_lock(&config_dir, options.switch.dry_run())?;
    switch::run(options.switch)
}

/// 生成済みローカル flake の全入力を最新の解決結果で lock し直す。
fn update_lock(config_dir: &Path, dry_run: bool) -> Result<()> {
    run_process(
        "nix",
        [
            OsString::from("flake"),
            OsString::from("update"),
            OsString::from("--flake"),
            config_dir.as_os_str().to_os_string(),
        ],
        dry_run,
    )
}

#[derive(Args)]
/// ローカル flake の入力を更新してから、既存の switch と同じ対象を適用する。
pub(crate) struct UpdateOptions {
    #[command(flatten)]
    switch: switch::SwitchOptions,
}
