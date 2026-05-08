use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{Result, command::step};

const SCENARIOS: &[&str] = &[
    "fresh-bootstrap",
    "second-user-home-manager",
    "darwin-switch-ya",
];

pub fn run(scenario: &str) -> Result<()> {
    validate_scenario(scenario)?;
    if env::var_os("GITHUB_ACTIONS").is_some() {
        run_guest_in_ci(scenario)
    } else {
        TartRunner::new(scenario)?.run()
    }
}

fn validate_scenario(scenario: &str) -> Result<()> {
    if scenario == "all" || SCENARIOS.contains(&scenario) {
        Ok(())
    } else {
        Err(format!("unsupported runtime scenario: {scenario}").into())
    }
}

fn run_guest_in_ci(scenario: &str) -> Result<()> {
    step("runtime scenario");
    let mut command = Command::new("cargo");
    command.args(["run", "--package", "dotfiles-runtime-guest", "--", scenario]);
    run_command(
        command,
        &format!("cargo run --package dotfiles-runtime-guest -- {scenario}"),
    )
}

struct TartRunner {
    scenario: String,
    repo_dir: PathBuf,
    image: String,
    vm_name: String,
    temp_dir: PathBuf,
    ssh_user: String,
    ssh_password: String,
    vm_created: bool,
    vm_started: bool,
    tart_child: Option<Child>,
}

impl TartRunner {
    fn new(scenario: &str) -> Result<Self> {
        validate_scenario(scenario)?;
        if env::consts::OS != "macos" {
            return Err("tart 検証は macOS ホストでのみ実行できます".into());
        }
        if env::consts::ARCH != "aarch64" {
            return Err("tart 検証は Apple Silicon ホストでのみ実行できます".into());
        }
        if command_in_path("tart").is_none() {
            return Err("tart が PATH にありません。nix develop で実行してください。".into());
        }
        if command_in_path("sshpass").is_none() {
            return Err("sshpass が PATH にありません。nix develop で実行してください。".into());
        }

        let repo_dir = env::var_os("DOTFILES_REPO_DIR")
            .map(PathBuf::from)
            .unwrap_or(env::current_dir()?);
        if !repo_dir.join("flake.nix").is_file() || !repo_dir.join(".git").is_dir() {
            return Err(format!(
                "repo_dir が dotfiles checkout を指していません: {}",
                repo_dir.display()
            )
            .into());
        }

        let image = env::var("DOTFILES_TART_IMAGE")
            .unwrap_or_else(|_| "ghcr.io/cirruslabs/macos-sequoia-vanilla:latest".to_string());
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let vm_name = env::var("DOTFILES_TART_VM_NAME")
            .unwrap_or_else(|_| format!("dotfiles-{scenario}-{timestamp}"));
        let temp_dir =
            env::temp_dir().join(format!("dotfiles-tart-{timestamp}-{}", std::process::id()));

        Ok(Self {
            scenario: scenario.to_string(),
            repo_dir,
            image,
            vm_name,
            temp_dir,
            ssh_user: env::var("DOTFILES_TART_SSH_USER").unwrap_or_else(|_| "admin".to_string()),
            ssh_password: env::var("DOTFILES_TART_SSH_PASSWORD")
                .unwrap_or_else(|_| "admin".to_string()),
            vm_created: false,
            vm_started: false,
            tart_child: None,
        })
    }

    fn run(mut self) -> Result<()> {
        fs::create_dir_all(&self.temp_dir)?;
        let guest_binary = self.build_guest_binary()?;

        step("tart clone");
        println!("cloning VM: {} -> {}", self.image, self.vm_name);
        run_plain("tart", ["clone", &self.image, &self.vm_name])?;
        self.vm_created = true;

        step("tart run");
        println!("starting VM: {}", self.vm_name);
        let tart_log = File::create(self.temp_dir.join("tart-run.log"))?;
        let tart_log_err = tart_log.try_clone()?;
        let child = Command::new("tart")
            .args([
                "run",
                "--no-graphics",
                &format!("--dir=repo:{}:ro", self.repo_dir.display()),
                &format!("--dir=guest:{}", self.temp_dir.display()),
                &self.vm_name,
            ])
            .stdout(Stdio::from(tart_log))
            .stderr(Stdio::from(tart_log_err))
            .spawn()?;
        self.vm_started = true;
        self.tart_child = Some(child);

        step("ssh wait");
        let ip = self.wait_for_ssh()?;
        println!("ssh ready: {}@{}", self.ssh_user, ip);

        step("copy runtime guest");
        self.scp(&ip, &guest_binary, "/tmp/dotfiles-runtime-guest")?;
        self.ssh(&ip, &["chmod", "+x", "/tmp/dotfiles-runtime-guest"])?;

        step("runtime scenario via tart");
        println!("running scenario: {}", self.scenario);
        self.ssh(&ip, &["/tmp/dotfiles-runtime-guest", &self.scenario])?;

        Ok(())
    }

    fn build_guest_binary(&self) -> Result<PathBuf> {
        step("build runtime guest");
        let mut command = Command::new("cargo");
        command
            .current_dir(&self.repo_dir)
            .args(["build", "--package", "dotfiles-runtime-guest"]);
        run_command(command, "cargo build --package dotfiles-runtime-guest")?;

        let binary = target_dir(&self.repo_dir).join("debug/dotfiles-runtime-guest");
        if !binary.is_file() {
            return Err(format!("runtime guest binary is missing: {}", binary.display()).into());
        }
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))?;
        Ok(binary)
    }

    fn wait_for_ssh(&mut self) -> Result<String> {
        for _ in 0..180 {
            if let Some(child) = self.tart_child.as_mut()
                && let Some(status) = child.try_wait()?
            {
                return Err(format!("tart run exited before ssh became ready: {status}").into());
            }
            if let Ok(ip) = read_plain("tart", ["ip", "--wait", "1", &self.vm_name]) {
                let ip = ip.trim().to_string();
                if !ip.is_empty() && self.ssh_status(&ip, &["/usr/bin/true"])?.success() {
                    return Ok(ip);
                }
            }
            thread::sleep(Duration::from_secs(2));
        }

        Err("ssh の起動待ちに失敗しました".into())
    }

    fn scp(&self, ip: &str, source: &Path, dest: &str) -> Result<()> {
        let target = format!("{}@{}:{dest}", self.ssh_user, ip);
        let mut command = password_command("scp", &self.ssh_password);
        command.args(ssh_options()).arg(source).arg(target);
        run_command(command, "scp runtime guest")
    }

    fn ssh(&self, ip: &str, remote_args: &[&str]) -> Result<()> {
        let status = self.ssh_status(ip, remote_args)?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("ssh command failed: {status}").into())
        }
    }

    fn ssh_status(&self, ip: &str, remote_args: &[&str]) -> Result<std::process::ExitStatus> {
        let mut command = password_command("ssh", &self.ssh_password);
        command
            .args(ssh_options())
            .arg(format!("{}@{}", self.ssh_user, ip))
            .args(remote_args);
        Ok(command.status()?)
    }
}

impl Drop for TartRunner {
    fn drop(&mut self) {
        if self.vm_started {
            let _ = Command::new("tart").args(["stop", &self.vm_name]).status();
        }
        if self.vm_created && env::var_os("DOTFILES_TART_KEEP_VM").is_none() {
            let _ = Command::new("tart")
                .args(["delete", &self.vm_name])
                .status();
        }
        let _ = fs::remove_dir_all(&self.temp_dir);
    }
}

fn target_dir(repo_dir: &Path) -> PathBuf {
    env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                repo_dir.join(path)
            }
        })
        .unwrap_or_else(|| repo_dir.join("target"))
}

fn run_plain<I, S>(program: &str, args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = arg_strings(args);
    let mut command = Command::new(program);
    command.args(&args);
    run_command(command, &display_command(program, &args))
}

fn read_plain<I, S>(program: &str, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = arg_strings(args);
    println!("$ {}", display_command(program, &args));
    let output = Command::new(program).args(&args).output()?;
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

fn run_command(mut command: Command, label: &str) -> Result<()> {
    println!("$ {label}");
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("command failed: {label}: {status}").into())
    }
}

fn password_command(program: &str, password: &str) -> Command {
    let mut command = Command::new("sshpass");
    command.args(["-e", program]).env("SSHPASS", password);
    command
}

fn ssh_options() -> [&'static str; 8] {
    [
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "UserKnownHostsFile=/dev/null",
        "-o",
        "PreferredAuthentications=password",
        "-o",
        "PubkeyAuthentication=no",
    ]
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

fn arg_strings<I, S>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter()
        .map(|arg| arg.as_ref().to_string_lossy().into_owned())
        .collect()
}
