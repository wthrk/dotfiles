use std::env;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const SCENARIOS: &[&str] = &[
    "fresh-bootstrap",
    "second-user-home-manager",
    "darwin-switch-ya",
];

fn main() -> ExitCode {
    match dispatch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch() -> Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.as_slice() {
        [scenario] => ScenarioRunner::new()?.run_scenario(scenario),
        _ => Err("usage: dotfiles-runtime-guest <scenario>".into()),
    }
}

fn validate_scenario(scenario: &str) -> Result<()> {
    if scenario == "all" || SCENARIOS.contains(&scenario) {
        Ok(())
    } else {
        Err(format!("unsupported runtime scenario: {scenario}").into())
    }
}

struct ScenarioEnv {
    workspace: PathBuf,
    runner_temp: PathBuf,
    nix_config: String,
}

impl ScenarioEnv {
    fn current() -> Result<Self> {
        let guest_workspace = PathBuf::from("/Volumes/My Shared Files/repo");
        let workspace = env::var_os("GITHUB_WORKSPACE")
            .map(PathBuf::from)
            .or_else(|| {
                guest_workspace
                    .join("flake.nix")
                    .is_file()
                    .then_some(guest_workspace)
            })
            .unwrap_or(env::current_dir()?);
        let runner_temp = env::var_os("RUNNER_TEMP")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                if workspace.starts_with("/Volumes/My Shared Files/repo") {
                    env::var_os("HOME")
                        .map(PathBuf::from)
                        .unwrap_or_else(env::temp_dir)
                        .join("runner-temp")
                } else {
                    env::temp_dir().join("dotfiles-runner-temp")
                }
            });
        let nix_config = env::var("NIX_CONFIG")
            .unwrap_or_else(|_| "experimental-features = nix-command flakes".to_string());
        fs::create_dir_all(&runner_temp)?;
        Ok(Self {
            workspace,
            runner_temp,
            nix_config,
        })
    }

    fn apply_to(&self, command: &mut Command) {
        command
            .current_dir(&self.workspace)
            .env("GITHUB_WORKSPACE", &self.workspace)
            .env("RUNNER_TEMP", &self.runner_temp)
            .env("NIX_CONFIG", &self.nix_config);
    }
}

struct ScenarioRunner {
    env: ScenarioEnv,
}

impl ScenarioRunner {
    fn new() -> Result<Self> {
        Ok(Self {
            env: ScenarioEnv::current()?,
        })
    }

    fn run_scenario(&self, scenario: &str) -> Result<()> {
        validate_scenario(scenario)?;
        if scenario == "all" {
            for name in SCENARIOS {
                println!("==> runtime scenario start: {name}");
                self.run_one(name)?;
                println!("==> runtime scenario ok: {name}");
            }
            return Ok(());
        }

        println!("==> runtime scenario start: {scenario}");
        self.run_one(scenario)?;
        println!("==> runtime scenario ok: {scenario}");
        Ok(())
    }

    fn run_one(&self, scenario: &str) -> Result<()> {
        match scenario {
            "fresh-bootstrap" => self.fresh_bootstrap(),
            "second-user-home-manager" => self.second_user_home_manager(),
            "darwin-switch-ya" => self.darwin_switch_ya(),
            _ => Err(format!("unsupported runtime scenario: {scenario}").into()),
        }
    }

    fn fresh_bootstrap(&self) -> Result<()> {
        self.ensure_nonempty("flake.lock")?;
        self.runner_info()?;

        if let Some(nix) = command_in_path("nix") {
            return Err(format!(
                "ゼロ状態の導入テストでは Nix 未導入を前提にします: {}",
                nix.display()
            )
            .into());
        }

        self.run("scripts/bootstrap.sh", &["--dry-run"])?;

        let bootstrap_dir = self.env.runner_temp.join("dotfiles-bootstrap");
        remove_path(&bootstrap_dir)?;
        self.run(
            "scripts/bootstrap.sh",
            &[
                "--repo",
                self.workspace_str().as_str(),
                "--dir",
                path_str(&bootstrap_dir).as_str(),
                "--mode",
                "darwin",
                "--no-switch",
            ],
        )?;

        ensure_dir(bootstrap_dir.join(".git"))?;
        ensure_nonempty_path(bootstrap_dir.join("flake.lock"))?;
        let checkout_head = self.read(
            "git",
            &["-C", path_str(&bootstrap_dir).as_str(), "rev-parse", "HEAD"],
        )?;
        let workspace_head = self.read(
            "git",
            &["-C", self.workspace_str().as_str(), "rev-parse", "HEAD"],
        )?;
        if checkout_head.trim() != workspace_head.trim() {
            return Err("bootstrap checkout HEAD が workspace と一致しません".into());
        }

        self.ensure_absent("/etc/bashrc.before-nix-darwin")?;
        self.ensure_absent("/etc/zshrc.before-nix-darwin")?;
        self.ensure_absent("/opt/homebrew/Library/Taps.before-nix-homebrew")?;
        self.ensure_absent("/usr/local/Library/Taps.before-nix-homebrew")?;

        let before = snapshot_system_paths()?;
        self.run(
            path_str(bootstrap_dir.join("scripts/bootstrap.sh")).as_str(),
            &[
                "--dir",
                path_str(&bootstrap_dir).as_str(),
                "--mode",
                "darwin",
                "--no-switch",
            ],
        )?;
        let after = snapshot_system_paths()?;
        if before != after {
            return Err(format!(
                "--no-switch が system path を変更しました\nbefore:\n{before}\nafter:\n{after}"
            )
            .into());
        }

        let missing_lock = self.env.runner_temp.join("dotfiles-missing-lock");
        self.copy_workspace_to(&missing_lock)?;
        fs::remove_file(missing_lock.join("flake.lock"))?;
        if self
            .status(
                path_str(self.env.workspace.join("scripts/bootstrap.sh")).as_str(),
                &[
                    "--dir",
                    path_str(&missing_lock).as_str(),
                    "--mode",
                    "darwin",
                    "--no-switch",
                ],
            )?
            .success()
        {
            return Err("flake.lock がない bootstrap が成功しました".into());
        }

        let broken_flake = self.env.runner_temp.join("dotfiles-broken-flake");
        self.copy_workspace_to(&broken_flake)?;
        append_text(broken_flake.join("flake.nix"), "\nthis is invalid nix\n")?;
        if self
            .status(
                path_str(self.env.workspace.join("scripts/bootstrap.sh")).as_str(),
                &[
                    "--dir",
                    path_str(&broken_flake).as_str(),
                    "--mode",
                    "darwin",
                    "--no-switch",
                ],
            )?
            .success()
        {
            return Err("壊れた flake の bootstrap が成功しました".into());
        }

        self.run("scripts/bootstrap.sh", &["--self-test"])
    }

    fn second_user_home_manager(&self) -> Result<()> {
        self.ensure_nonempty("flake.lock")?;
        self.runner_info()?;

        let bootstrap_dir = self.env.runner_temp.join("dotfiles-bootstrap");
        remove_path(&bootstrap_dir)?;
        self.run(
            "scripts/bootstrap.sh",
            &[
                "--repo",
                self.workspace_str().as_str(),
                "--dir",
                path_str(&bootstrap_dir).as_str(),
                "--mode",
                "darwin",
                "--no-switch",
            ],
        )?;

        ensure_local_user(
            self,
            "dotfilesci",
            "Dotfiles CI",
            "DotfilesCI-Temp-2026!",
            "/Users/dotfilesci",
            "20",
        )?;
        let _ = self.status("sudo", &["createhomedir", "-c", "-u", "dotfilesci"])?;
        self.run("sudo", &["mkdir", "-p", "/Users/dotfilesci"])?;
        self.run("sudo", &["chown", "dotfilesci:staff", "/Users/dotfilesci"])?;
        self.run("sudo", &["rm", "-rf", "/Users/dotfilesci/.dotfiles"])?;

        self.run_sudo_user(
            "dotfilesci",
            &dotfilesci_env(&self.env.nix_config),
            path_str(self.env.workspace.join("scripts/bootstrap.sh")).as_str(),
            &[
                "--repo",
                self.workspace_str().as_str(),
                "--dir",
                "/Users/dotfilesci/.dotfiles",
                "--flake",
                "dotfilesci",
                "--mode",
                "home-manager",
                "--run-switch",
            ],
        )?;

        ensure_dir("/Users/dotfilesci/.dotfiles/.git")?;
        ensure_nonempty_path("/Users/dotfilesci/.dotfiles/flake.lock")?;
        ensure_exists("/Users/dotfilesci/.nix-profile")?;
        assert_managed_links(
            "/Users/dotfilesci",
            &[".config/zsh", ".config/nvim", ".zshrc", ".zshenv"],
        )?;
        self.run_sudo_user(
            "dotfilesci",
            &dotfilesci_env(&self.env.nix_config),
            "/nix/var/nix/profiles/default/bin/nix",
            &[
                "--extra-experimental-features",
                "nix-command flakes",
                "eval",
                "--no-update-lock-file",
                "/Users/dotfilesci/.dotfiles#homeConfigurations.dotfilesci.activationPackage.drvPath",
            ],
        )
    }

    fn darwin_switch_ya(&self) -> Result<()> {
        self.ensure_nonempty("flake.lock")?;
        self.runner_info()?;

        let ya_checkout = PathBuf::from("/Users/ya/.dotfiles");
        ensure_local_user(self, "ya", "ya", "Ya-Temp-2026!", "/Users/ya", "80")?;
        let _ = self.status("sudo", &["createhomedir", "-c", "-u", "ya"])?;
        self.run("sudo", &["mkdir", "-p", "/Users/ya"])?;
        self.run("sudo", &["chown", "ya:staff", "/Users/ya"])?;
        self.run_sudo_user(
            "ya",
            &ya_env(&self.env.nix_config),
            "mkdir",
            &["-p", "/Users/ya/Library/LaunchAgents"],
        )?;
        self.run_sudo_user(
            "ya",
            &ya_env(&self.env.nix_config),
            "rm",
            &["-f", "/Users/ya/Library/LaunchAgents/org.nix.colima.plist"],
        )?;
        self.run_sudo_user(
            "ya",
            &ya_env(&self.env.nix_config),
            "ln",
            &[
                "-s",
                "/nix/store/missing-org.nix.colima.plist",
                "/Users/ya/Library/LaunchAgents/org.nix.colima.plist",
            ],
        )?;
        if Path::new("/opt/homebrew").is_dir() {
            self.run("sudo", &["chown", "-R", "ya:admin", "/opt/homebrew"])?;
        }

        self.run(
            "sudo",
            &[
                "git",
                "config",
                "--global",
                "--add",
                "safe.directory",
                self.workspace_str().as_str(),
            ],
        )?;
        self.copy_workspace_to_with_sudo(&ya_checkout)?;
        self.run(
            "sudo",
            &["chown", "-R", "ya:staff", path_str(&ya_checkout).as_str()],
        )?;
        self.run(
            "sudo",
            &[
                "git",
                "config",
                "--global",
                "--add",
                "safe.directory",
                path_str(&ya_checkout).as_str(),
            ],
        )?;
        ensure_dir(ya_checkout.join(".git"))?;
        ensure_nonempty_path(ya_checkout.join("flake.nix"))?;
        ensure_nonempty_path(ya_checkout.join("flake.lock"))?;

        self.run(
            "scripts/bootstrap.sh",
            &[
                "--repo",
                self.workspace_str().as_str(),
                "--dir",
                path_str(&ya_checkout).as_str(),
                "--mode",
                "darwin",
                "--run-switch",
            ],
        )?;

        let nix = "/nix/var/nix/profiles/default/bin/nix";
        ensure_executable(nix)?;
        self.run_as_ya(
            nix,
            &[
                "--extra-experimental-features",
                "nix-command flakes",
                "flake",
                "check",
                "--no-update-lock-file",
                path_str(&ya_checkout).as_str(),
            ],
        )?;
        self.run_as_ya(
            nix,
            &[
                "--extra-experimental-features",
                "nix-command flakes",
                "eval",
                "--no-update-lock-file",
                "/Users/ya/.dotfiles#homeConfigurations.default.activationPackage.drvPath",
            ],
        )?;
        self.run_as_ya(
            nix,
            &[
                "--extra-experimental-features",
                "nix-command flakes",
                "eval",
                "--no-update-lock-file",
                "/Users/ya/.dotfiles#darwinConfigurations.default.system",
            ],
        )?;
        ensure_dir("/etc/profiles/per-user/ya")?;
        assert_managed_links(
            "/Users/ya",
            &[".config/zsh", ".config/nvim", ".zshrc", ".zshenv"],
        )?;
        self.run_as_ya(
            "/etc/profiles/per-user/ya/bin/zsh",
            &[
                "-il",
                "-c",
                r#"
set -euo pipefail
for tool in git gh jq rg fzf atuin zoxide nvim; do
  tool_path="$(command -v "$tool")"
  real_tool_path="${tool_path:A}"
  case "$real_tool_path" in
    /nix/store/*) ;;
    *)
      echo "$tool resolved outside the Nix store: $tool_path -> $real_tool_path" >&2
      exit 1
      ;;
  esac
done
"#,
            ],
        )?;

        let taps = self.read_as_ya("/opt/homebrew/bin/brew", &["tap"])?;
        expect_line(&taps, "azure/bicep")?;
        expect_line(&taps, "hashicorp/tap")?;
        let formulae = self.read_as_ya("/opt/homebrew/bin/brew", &["list", "--formula"])?;
        if formulae.lines().any(|line| line == "packer") {
            return Err("packer formula remains after Homebrew cleanup".into());
        }

        assert_colima_plist("/Users/ya/Library/LaunchAgents/org.nix.colima.plist")?;
        ensure_absent_path("/Users/ya/Library/LaunchAgents/homebrew.mxcl.colima.plist")?;

        let uid = self.read("id", &["-u", "ya"])?;
        let uid = uid.trim();
        if let Some(output) = self.read_as_ya_status(
            "launchctl",
            &["print", &format!("gui/{uid}/org.nix.colima")],
        )? && (!output.contains("/nix/store/") || !output.contains("/bin/colima"))
        {
            return Err("org.nix.colima launchd output does not point to Nix colima".into());
        }
        if self
            .status_as_ya(
                "launchctl",
                &["print", &format!("gui/{uid}/homebrew.mxcl.colima")],
            )?
            .success()
        {
            return Err("homebrew.mxcl.colima is still loaded".into());
        }

        Ok(())
    }

    fn runner_info(&self) -> Result<()> {
        self.run("sw_vers", &[])?;
        self.run("uname", &["-a"])?;
        self.run("id", &[])?;
        self.run("xcode-select", &["-p"])
    }

    fn copy_workspace_to(&self, target: &Path) -> Result<()> {
        remove_path(target)?;
        fs::create_dir_all(target)?;
        self.run(
            "rsync",
            &[
                "-a",
                "--exclude",
                "/.direnv/",
                "--exclude",
                "/target/",
                format!("{}/", self.env.workspace.display()).as_str(),
                format!("{}/", target.display()).as_str(),
            ],
        )
    }

    fn copy_workspace_to_with_sudo(&self, target: &Path) -> Result<()> {
        self.run("sudo", &["rm", "-rf", path_str(target).as_str()])?;
        self.run("sudo", &["mkdir", "-p", path_str(target).as_str()])?;
        self.run(
            "sudo",
            &[
                "rsync",
                "-a",
                "--exclude",
                "/.direnv/",
                "--exclude",
                "/target/",
                format!("{}/", self.env.workspace.display()).as_str(),
                format!("{}/", target.display()).as_str(),
            ],
        )
    }

    fn ensure_nonempty(&self, path: &str) -> Result<()> {
        ensure_nonempty_path(self.env.workspace.join(path))
    }

    fn ensure_absent(&self, path: &str) -> Result<()> {
        ensure_absent_path(path)
    }

    fn workspace_str(&self) -> String {
        path_str(&self.env.workspace)
    }

    fn run(&self, program: &str, args: &[&str]) -> Result<()> {
        run_with_env(Some(&self.env), program, args)
    }

    fn read(&self, program: &str, args: &[&str]) -> Result<String> {
        read_with_env(Some(&self.env), program, args)
    }

    fn status(&self, program: &str, args: &[&str]) -> Result<std::process::ExitStatus> {
        status_with_env(Some(&self.env), program, args)
    }

    fn run_sudo_user(
        &self,
        user: &str,
        envs: &[(String, String)],
        program: &str,
        args: &[&str],
    ) -> Result<()> {
        run_with_env(
            Some(&self.env),
            "sudo",
            sudo_user_args(user, envs, program, args),
        )
    }

    fn run_as_ya(&self, program: &str, args: &[&str]) -> Result<()> {
        self.run_sudo_user("ya", &ya_env(&self.env.nix_config), program, args)
    }

    fn read_as_ya(&self, program: &str, args: &[&str]) -> Result<String> {
        read_with_env(
            Some(&self.env),
            "sudo",
            sudo_user_args("ya", &ya_env(&self.env.nix_config), program, args),
        )
    }

    fn status_as_ya(&self, program: &str, args: &[&str]) -> Result<std::process::ExitStatus> {
        status_with_env(
            Some(&self.env),
            "sudo",
            sudo_user_args("ya", &ya_env(&self.env.nix_config), program, args),
        )
    }

    fn read_as_ya_status(&self, program: &str, args: &[&str]) -> Result<Option<String>> {
        read_status_with_env(
            Some(&self.env),
            "sudo",
            sudo_user_args("ya", &ya_env(&self.env.nix_config), program, args),
        )
    }
}

fn run_with_env<I, S>(env: Option<&ScenarioEnv>, program: &str, args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = arg_strings(args);
    let mut command = Command::new(program);
    command.args(&args);
    if let Some(env) = env {
        env.apply_to(&mut command);
    }
    run_command(command, &display_command(program, &args))
}

fn status_with_env<I, S>(
    env: Option<&ScenarioEnv>,
    program: &str,
    args: I,
) -> Result<std::process::ExitStatus>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = arg_strings(args);
    println!("$ {}", display_command(program, &args));
    let mut command = Command::new(program);
    command.args(&args);
    if let Some(env) = env {
        env.apply_to(&mut command);
    }
    Ok(command.status()?)
}

fn read_with_env<I, S>(env: Option<&ScenarioEnv>, program: &str, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = arg_strings(args);
    println!("$ {}", display_command(program, &args));
    let mut command = Command::new(program);
    command.args(&args);
    if let Some(env) = env {
        env.apply_to(&mut command);
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(format!(
            "command failed: {}\n{}",
            display_command(program, &args),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn read_status_with_env<I, S>(
    env: Option<&ScenarioEnv>,
    program: &str,
    args: I,
) -> Result<Option<String>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = arg_strings(args);
    println!("$ {}", display_command(program, &args));
    let mut command = Command::new(program);
    command.args(&args);
    if let Some(env) = env {
        env.apply_to(&mut command);
    }
    let output = command.output()?;
    if output.status.success() {
        Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
    } else {
        Ok(None)
    }
}

fn run_command(mut command: Command, label: &str) -> Result<()> {
    println!("$ {label}");
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("command failed: {label}: {status}").into())
    }
}

fn display_command(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(shellish_quote)
        .collect::<Vec<_>>()
        .join(" ")
}

fn shellish_quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "-_./:=@+".contains(ch))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn sudo_user_args(
    user: &str,
    envs: &[(String, String)],
    program: &str,
    args: &[&str],
) -> Vec<String> {
    let mut result = vec![
        "-H".to_string(),
        "-u".to_string(),
        user.to_string(),
        "env".to_string(),
    ];
    result.extend(envs.iter().map(|(key, value)| format!("{key}={value}")));
    result.push(program.to_string());
    result.extend(args.iter().map(|arg| (*arg).to_string()));
    result
}

fn dotfilesci_env(nix_config: &str) -> Vec<(String, String)> {
    vec![
        ("HOME".to_string(), "/Users/dotfilesci".to_string()),
        ("USER".to_string(), "dotfilesci".to_string()),
        ("LOGNAME".to_string(), "dotfilesci".to_string()),
        ("SHELL".to_string(), "/bin/zsh".to_string()),
        ("NIX_CONFIG".to_string(), nix_config.to_string()),
    ]
}

fn ya_env(nix_config: &str) -> Vec<(String, String)> {
    vec![
        ("HOME".to_string(), "/Users/ya".to_string()),
        ("USER".to_string(), "ya".to_string()),
        ("LOGNAME".to_string(), "ya".to_string()),
        (
            "SHELL".to_string(),
            "/etc/profiles/per-user/ya/bin/zsh".to_string(),
        ),
        (
            "PATH".to_string(),
            "/etc/profiles/per-user/ya/bin:/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin:/usr/bin:/bin:/usr/sbin:/sbin".to_string(),
        ),
        ("NIX_CONFIG".to_string(), nix_config.to_string()),
    ]
}

fn ensure_local_user(
    runner: &ScenarioRunner,
    user: &str,
    full_name: &str,
    password: &str,
    home: &str,
    primary_gid: &str,
) -> Result<()> {
    if runner.status("id", &[user])?.success() {
        return Ok(());
    }

    let uid = next_uid(runner)?;
    runner.run("sudo", &["dscl", ".", "-create", &format!("/Users/{user}")])?;
    runner.run(
        "sudo",
        &[
            "dscl",
            ".",
            "-create",
            &format!("/Users/{user}"),
            "UserShell",
            "/bin/zsh",
        ],
    )?;
    runner.run(
        "sudo",
        &[
            "dscl",
            ".",
            "-create",
            &format!("/Users/{user}"),
            "RealName",
            full_name,
        ],
    )?;
    runner.run(
        "sudo",
        &[
            "dscl",
            ".",
            "-create",
            &format!("/Users/{user}"),
            "UniqueID",
            &uid.to_string(),
        ],
    )?;
    runner.run(
        "sudo",
        &[
            "dscl",
            ".",
            "-create",
            &format!("/Users/{user}"),
            "PrimaryGroupID",
            primary_gid,
        ],
    )?;
    runner.run(
        "sudo",
        &[
            "dscl",
            ".",
            "-create",
            &format!("/Users/{user}"),
            "NFSHomeDirectory",
            home,
        ],
    )?;
    runner.run(
        "sudo",
        &["dscl", ".", "-passwd", &format!("/Users/{user}"), password],
    )
}

fn next_uid(runner: &ScenarioRunner) -> Result<u32> {
    let output = runner.read("dscl", &[".", "-list", "/Users", "UniqueID"])?;
    let mut uid = 501;
    for line in output.lines() {
        if let Some(value) = line.split_whitespace().nth(1)
            && let Ok(existing) = value.parse::<u32>()
            && existing >= uid
        {
            uid = existing + 1;
        }
    }
    Ok(uid)
}

fn snapshot_system_paths() -> Result<String> {
    let mut snapshot = String::new();
    for path in [
        "/etc/bashrc",
        "/etc/zshrc",
        "/opt/homebrew/Library/Taps",
        "/usr/local/Library/Taps",
    ] {
        match fs::symlink_metadata(path) {
            Ok(meta) => {
                snapshot.push_str(&format!(
                    "{} {} {} {} {} {:o}\n",
                    path,
                    meta.size(),
                    meta.mtime(),
                    meta.uid(),
                    meta.gid(),
                    meta.mode()
                ));
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                snapshot.push_str(&format!("missing {path}\n"));
            }
            Err(err) => return Err(err.into()),
        }
    }
    Ok(snapshot)
}

fn assert_managed_links(home: &str, paths: &[&str]) -> Result<()> {
    for relative in paths {
        let path = Path::new(home).join(relative);
        let meta = fs::symlink_metadata(&path)?;
        if !meta.file_type().is_symlink() {
            return Err(format!("{} is not a symlink", path.display()).into());
        }
        let target = fs::read_link(&path)?;
        if !target.starts_with("/nix/store") {
            return Err(format!(
                "{} does not point into /nix/store: {}",
                path.display(),
                target.display()
            )
            .into());
        }
    }
    Ok(())
}

fn assert_colima_plist(path: &str) -> Result<()> {
    let plist = Path::new(path);
    ensure_exists(plist)?;
    let target = if fs::symlink_metadata(plist)?.file_type().is_symlink() {
        let link = fs::read_link(plist)?;
        if link.is_absolute() {
            link
        } else {
            plist.parent().unwrap_or(Path::new("/")).join(link)
        }
    } else {
        plist.to_path_buf()
    };
    ensure_nonempty_path(&target)?;
    let text = fs::read_to_string(&target)?;
    if !text.contains("/nix/store/") || !text.contains("/bin/colima") {
        return Err(format!("{} does not reference Nix colima", target.display()).into());
    }
    Ok(())
}

fn expect_line(text: &str, expected: &str) -> Result<()> {
    if text.lines().any(|line| line == expected) {
        Ok(())
    } else {
        Err(format!("expected line not found: {expected}").into())
    }
}

fn append_text(path: impl AsRef<Path>, text: &str) -> Result<()> {
    let mut file = OpenOptions::new().append(true).open(path)?;
    file.write_all(text.as_bytes())?;
    Ok(())
}

fn remove_path(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path)?,
        Ok(_) => fs::remove_file(path)?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    Ok(())
}

fn ensure_exists(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    if path.exists() {
        Ok(())
    } else {
        Err(format!("{} is missing", path.display()).into())
    }
}

fn ensure_absent_path(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    if path.exists() || fs::symlink_metadata(path).is_ok() {
        Err(format!("{} exists", path.display()).into())
    } else {
        Ok(())
    }
}

fn ensure_dir(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    if path.is_dir() {
        Ok(())
    } else {
        Err(format!("{} is not a directory", path.display()).into())
    }
}

fn ensure_nonempty_path(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    if path.metadata()?.len() > 0 {
        Ok(())
    } else {
        Err(format!("{} is empty", path.display()).into())
    }
}

fn ensure_executable(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    let meta = path.metadata()?;
    if meta.permissions().mode() & 0o111 != 0 {
        Ok(())
    } else {
        Err(format!("{} is not executable", path.display()).into())
    }
}

fn command_in_path(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let path = PathBuf::from(name);
        return path.is_file().then_some(path);
    }

    let path_var = env::var_os("PATH")?;
    env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

fn path_str(path: impl AsRef<Path>) -> String {
    path.as_ref().display().to_string()
}

fn arg_strings<I, S>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter()
        .map(|arg| arg.as_ref().to_string_lossy().into_owned())
        .collect()
}
