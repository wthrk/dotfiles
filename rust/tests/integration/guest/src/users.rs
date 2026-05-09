use crate::{Result, scenario::ScenarioRunner};

pub(crate) fn ensure_local_user(
    runner: &ScenarioRunner,
    user: &str,
    full_name: &str,
    password: &str,
    admin: bool,
) -> Result<()> {
    if runner.status("id", &[user])?.success() {
        return Ok(());
    }

    let mut args = vec![
        "sysadminctl",
        "-addUser",
        user,
        "-fullName",
        full_name,
        "-password",
        password,
        "-shell",
        "/bin/zsh",
    ];
    if admin {
        args.push("-admin");
    }
    runner.run("sudo", &args)
}
