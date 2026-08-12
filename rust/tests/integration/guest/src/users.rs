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

/// ハーネスが作ったアカウントに、このゲスト内だけの非対話 sudo を与える。
///
/// bootstrap の sudo 判定と nix-darwin 適用は実際に `sudo` を通る。シナリオは端末を持たない
/// 実行環境で走るため、パスワード入力を求められた時点でその経路を検証できない。付与対象は
/// 同じハーネスが作った使い捨てアカウントに限り、sub user 側には付与しない。sub user が
/// sudo を要求しないことは検証対象そのものなので、与えると退行を隠す。
pub(crate) fn grant_noninteractive_sudo(runner: &ScenarioRunner, user: &str) -> Result<()> {
    let path = format!("/etc/sudoers.d/dotfiles-integration-{user}");
    let script = format!(
        "set -eu; printf '%s ALL=(ALL) NOPASSWD: ALL\\n' '{user}' > '{path}'; chmod 0440 '{path}'; /usr/sbin/visudo -c -f '{path}'"
    );
    runner.run("sudo", &["/bin/sh", "-c", script.as_str()])
}
