use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::bail;
use clap::{Parser, ValueEnum};
use dotfiles_core::{command as command_format, path::find_executable};

type Result<T> = dotfiles_core::Result<T>;

#[derive(Parser)]
struct Args {
    #[arg(value_enum)]
    scenario: Option<RuntimeScenario>,
    #[arg(long, env = "GITHUB_ACTIONS", hide = true)]
    github_actions: Option<String>,
    #[arg(long, env = "DOTFILES_REPO_DIR", value_name = "PATH")]
    repo_dir: Option<PathBuf>,
    #[arg(
        long,
        env = "DOTFILES_TART_IMAGE",
        default_value = "ghcr.io/cirruslabs/macos-sequoia-vanilla:latest"
    )]
    image: String,
    #[arg(long, env = "DOTFILES_TART_VM_NAME")]
    vm_name: Option<String>,
    #[arg(long, env = "DOTFILES_TART_SSH_USER", default_value = "admin")]
    ssh_user: String,
    #[arg(long, env = "DOTFILES_TART_SSH_PASSWORD", default_value = "admin")]
    ssh_password: String,
    #[arg(long, env = "DOTFILES_TART_KEEP_VM", hide = true)]
    keep_vm: Option<String>,
    #[arg(long, env = "CARGO_TARGET_DIR", value_name = "PATH", hide = true)]
    cargo_target_dir: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum RuntimeScenario {
    Full,
}

fn main() -> std::process::ExitCode {
    match run(Args::parse()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<()> {
    if args.github_actions.is_some() {
        run_guest_in_ci(args.scenario())
    } else {
        TartRunner::new(args)?.run()
    }
}

impl Args {
    fn scenario(&self) -> RuntimeScenario {
        self.scenario.unwrap_or(RuntimeScenario::Full)
    }
}

fn run_guest_in_ci(scenario: RuntimeScenario) -> Result<()> {
    step("integration test guest");
    let mut command = Command::new("cargo");
    command.args(["run", "--package", "dotfiles-integration-test-guest"]);
    match scenario {
        RuntimeScenario::Full => run_command(
            command,
            "cargo run --package dotfiles-integration-test-guest",
        ),
    }
}

struct TartRunner {
    scenario: RuntimeScenario,
    repo_dir: PathBuf,
    image: String,
    vm_name: String,
    temp_dir: PathBuf,
    ssh_user: String,
    ssh_password: String,
    keep_vm: bool,
    cargo_target_dir: Option<PathBuf>,
    vm_created: bool,
    vm_started: bool,
    tart_child: Option<Child>,
}

impl TartRunner {
    fn new(args: Args) -> Result<Self> {
        if env::consts::OS != "macos" {
            bail!("tart 検証は macOS ホストでのみ実行できます");
        }
        if env::consts::ARCH != "aarch64" {
            bail!("tart 検証は Apple Silicon ホストでのみ実行できます");
        }
        if find_executable("tart").is_none() {
            bail!("tart が PATH にありません。nix develop で実行してください。");
        }
        if find_executable("sshpass").is_none() {
            bail!("sshpass が PATH にありません。nix develop で実行してください。");
        }
        if find_executable("nc").is_none() {
            bail!("nc が PATH にありません。nix develop で実行してください。");
        }

        let scenario = args.scenario();
        let repo_dir = args.repo_dir.unwrap_or(env::current_dir()?);
        if !repo_dir.join("flake.nix").is_file() || !repo_dir.join(".git").exists() {
            bail!(
                "repo_dir が dotfiles checkout を指していません: {}",
                repo_dir.display()
            );
        }

        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let vm_name = args
            .vm_name
            .unwrap_or_else(|| format!("dotfiles-full-{timestamp}"));
        let temp_dir =
            env::temp_dir().join(format!("dotfiles-tart-{timestamp}-{}", std::process::id()));

        Ok(Self {
            scenario,
            repo_dir,
            image: args.image,
            vm_name,
            temp_dir,
            ssh_user: args.ssh_user,
            ssh_password: args.ssh_password,
            keep_vm: args.keep_vm.is_some(),
            cargo_target_dir: args.cargo_target_dir,
            vm_created: false,
            vm_started: false,
            tart_child: None,
        })
    }

    fn run(mut self) -> Result<()> {
        fs::create_dir_all(&self.temp_dir)?;
        let guest_binary = self.build_guest_binary()?;
        let mounted_guest_binary = self.temp_dir.join("dotfiles-integration-test-guest");
        fs::copy(&guest_binary, &mounted_guest_binary)?;
        fs::set_permissions(&mounted_guest_binary, fs::Permissions::from_mode(0o755))?;

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
        let session = self.connect_ssh()?;
        println!("ssh ready: {}", session.destination());

        step("copy integration test guest");
        session.run(
            &["/bin/sh -c \"cp '/Volumes/My Shared Files/guest/dotfiles-integration-test-guest' /tmp/dotfiles-integration-test-guest\""],
        )?;
        session.run(&["chmod", "+x", "/tmp/dotfiles-integration-test-guest"])?;

        step("integration test scenario via tart");
        match self.scenario {
            RuntimeScenario::Full => {
                println!("running full runtime scenario");
                session.run(&["/tmp/dotfiles-integration-test-guest"])?;
            }
        }

        Ok(())
    }

    fn build_guest_binary(&self) -> Result<PathBuf> {
        step("build integration test guest");
        let mut command = Command::new("cargo");
        command.current_dir(&self.repo_dir).args([
            "build",
            "--package",
            "dotfiles-integration-test-guest",
        ]);
        run_command(
            command,
            "cargo build --package dotfiles-integration-test-guest",
        )?;

        let binary = target_dir(&self.repo_dir, self.cargo_target_dir.as_deref())
            .join("debug/dotfiles-integration-test-guest");
        if !binary.is_file() {
            bail!(
                "integration test guest binary is missing: {}",
                binary.display()
            );
        }
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))?;
        Ok(binary)
    }

    fn wait_for_ssh(&mut self) -> Result<String> {
        for _ in 0..180 {
            if let Some(child) = self.tart_child.as_mut()
                && let Some(status) = child.try_wait()?
            {
                bail!("tart run exited before ssh became ready: {status}");
            }
            if let Ok(ip) = read_plain("tart", ["ip", "--wait", "1", &self.vm_name]) {
                let ip = ip.trim().to_string();
                if !ip.is_empty() && ssh_port_ready(&ip)? {
                    thread::sleep(Duration::from_secs(30));
                    return Ok(ip);
                }
            }
            thread::sleep(Duration::from_secs(2));
        }

        bail!("ssh の起動待ちに失敗しました")
    }

    fn connect_ssh(&mut self) -> Result<SshSession> {
        let ip = self.wait_for_ssh()?;
        let mut session = SshSession {
            user: self.ssh_user.clone(),
            control_path: self.ssh_control_path(),
            master: self.spawn_ssh_master(&ip)?,
            ip,
        };
        session.wait_until_ready()?;
        Ok(session)
    }

    fn spawn_ssh_master(&self, ip: &str) -> Result<Child> {
        let control_path = self.ssh_control_path();
        let mut command = password_command("ssh", &self.ssh_password);
        command
            .args(ssh_options())
            .args(["-M", "-S", &control_path, "-N", "-T"])
            .arg(format!("{}@{}", self.ssh_user, ip))
            .stdin(Stdio::null());
        println!("$ ssh control master");
        Ok(command.spawn()?)
    }

    fn ssh_control_path(&self) -> String {
        self.temp_dir.join("ssh-control").display().to_string()
    }
}

fn step(label: &str) {
    println!("==> {label}");
}

struct SshSession {
    user: String,
    control_path: String,
    master: Child,
    ip: String,
}

impl SshSession {
    fn wait_until_ready(&mut self) -> Result<()> {
        for _ in 0..30 {
            if let Some(status) = self.master.try_wait()? {
                bail!("ssh control master exited before ready: {status}");
            }
            if !Path::new(&self.control_path).exists() {
                thread::sleep(Duration::from_secs(1));
                continue;
            }
            if self
                .status(&["/usr/bin/true"])
                .is_ok_and(|status| status.success())
            {
                return Ok(());
            }
            thread::sleep(Duration::from_secs(1));
        }

        bail!("ssh control master did not become ready")
    }

    fn run(&self, remote_args: &[&str]) -> Result<()> {
        let status = self.status(remote_args)?;
        if status.success() {
            Ok(())
        } else {
            bail!("ssh command failed: {status}")
        }
    }

    fn status(&self, remote_args: &[&str]) -> Result<std::process::ExitStatus> {
        if !Path::new(&self.control_path).exists() {
            bail!("ssh control socket is missing: {}", self.control_path);
        }
        let mut command = Command::new("ssh");
        command
            .args(session_ssh_options())
            .args(["-S", &self.control_path])
            .arg(self.destination())
            .args(remote_args);
        Ok(command.status()?)
    }

    fn destination(&self) -> String {
        format!("{}@{}", self.user, self.ip)
    }
}

impl Drop for SshSession {
    fn drop(&mut self) {
        let _ = Command::new("ssh")
            .args(session_ssh_options())
            .args(["-S", &self.control_path, "-O", "exit"])
            .arg(self.destination())
            .status();
        if self.master.try_wait().ok().flatten().is_none() {
            let _ = self.master.kill();
            let _ = self.master.wait();
        }
    }
}

impl Drop for TartRunner {
    fn drop(&mut self) {
        if self.vm_started {
            let _ = Command::new("tart").args(["stop", &self.vm_name]).status();
        }
        if self.vm_created && !self.keep_vm {
            let _ = Command::new("tart")
                .args(["delete", &self.vm_name])
                .status();
        }
        let _ = fs::remove_dir_all(&self.temp_dir);
    }
}

fn target_dir(repo_dir: &Path, cargo_target_dir: Option<&Path>) -> PathBuf {
    cargo_target_dir
        .map(|path| {
            if path.is_absolute() {
                path.to_path_buf()
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
    let args = command_format::os_strings(args);
    let mut command = Command::new(program);
    command.args(&args);
    run_command(command, &command_format::display(program, &args))
}

fn read_plain<I, S>(program: &str, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = command_format::os_strings(args);
    println!("$ {}", command_format::display(program, &args));
    let output = Command::new(program).args(&args).output()?;
    if !output.status.success() {
        bail!(
            "command failed: {}\n{}",
            command_format::display(program, &args),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn ssh_port_ready(ip: &str) -> Result<bool> {
    Ok(Command::new("nc")
        .args(["-G", "1", "-z", ip, "22"])
        .status()?
        .success())
}

fn run_command(mut command: Command, label: &str) -> Result<()> {
    println!("$ {label}");
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        bail!("command failed: {label}: {status}")
    }
}

fn password_command(program: &str, password: &str) -> Command {
    let mut command = Command::new("sshpass");
    command.args(["-e", program]).env("SSHPASS", password);
    command
}

fn ssh_options() -> [&'static str; 10] {
    [
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "UserKnownHostsFile=/dev/null",
        "-o",
        "PreferredAuthentications=password",
        "-o",
        "PubkeyAuthentication=no",
        "-o",
        "NumberOfPasswordPrompts=1",
    ]
}

fn session_ssh_options() -> [&'static str; 6] {
    [
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "UserKnownHostsFile=/dev/null",
        "-o",
        "BatchMode=yes",
    ]
}
