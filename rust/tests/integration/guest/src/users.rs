//! 統合シナリオで使う macOS ローカルユーザーを用意する。

use crate::{Result, scenario::ScenarioRunner};

/// 既存なら再利用し、なければ sysadminctl で指定された shell/password/admin 権限のユーザーを作る。
pub(crate) fn ensure_local_user(
    runner: &ScenarioRunner,
    user: &str,
    full_name: &str,
    password: &str,
    admin: bool,
) -> Result<()> {
    // 既存ユーザーは許容する。前回失敗時のアカウントが残るゲストに対しても
    // シナリオを再実行できるようにする。
    if runner.status("id", &[user])?.success() {
        return Ok(());
    }

    let args = [
        "sysadminctl",
        "-addUser",
        user,
        "-fullName",
        full_name,
        "-password",
        password,
        "-shell",
        "/bin/zsh",
    ]
    .into_iter()
    .chain(admin.then_some("-admin"))
    .collect::<Vec<_>>();
    runner.run("sudo", &args)
}
