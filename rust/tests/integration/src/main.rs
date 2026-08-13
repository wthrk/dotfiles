//! Tart VM をゲストとして用意し、その中で実行時統合シナリオを起動する。
//!
//! ゲスト内で走る手順は `.github/workflows/runtime-integration.yml` が runner に対して行うものと
//! 同じにする。checkout も guest 実行器のビルドもゲスト内で行い、ホストの成果物は VM へ持ち込まない。
//! ホスト側が持つのは VM の生成と SSH 接続だけである。
//!
//! SSH はパスワード認証を制御マスターの確立時だけに限定し、その後の手順は同じ接続で流す。

use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::bail;
use clap::{Parser, ValueEnum};
use dotfiles_core::{command as command_format, path::find_executable};

type Result<T> = dotfiles_core::Result<T>;

/// ゲストが checkout する先。CI の `actions/checkout` が作る作業ツリーに対応する。
const GUEST_REPO_DIR: &str = "$HOME/dotfiles";

/// ゲストが checkout 元にする repository。bootstrap が参照する flake と同じ出所にする。
const REPO_URL: &str = "https://github.com/wthrk/dotfiles.git";

/// CI の `dtolnay/rust-toolchain@stable` に対応する、ゲスト内での Rust toolchain 導入。
const INSTALL_RUST_TOOLCHAIN: &str = "curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs \
     | sh -s -- -y --profile minimal --default-toolchain stable";

/// CI の `reset Homebrew installation` step と同じ除去。
const RESET_HOMEBREW: &str = "sudo rm -rf /opt/homebrew /usr/local/Homebrew /usr/local/Caskroom \
     /usr/local/Cellar /usr/local/Frameworks /usr/local/opt /usr/local/var/homebrew \
     /usr/local/bin/brew";

#[derive(Parser)]
/// Tart イメージ、VM 名、SSH 認証情報、検証対象 commit を clap/env から受け取る。
struct Args {
    #[arg(value_enum)]
    scenario: Option<RuntimeScenario>,
    /// ゲストが checkout し、bootstrap が `github:wthrk/dotfiles/<sha>` として参照する commit。
    #[arg(long, env = "DOTFILES_TEST_SOURCE_HASH")]
    source_hash: String,
    #[arg(long, env = "DOTFILES_TART_IMAGE", default_value = "sequoia-vanilla")]
    image: String,
    #[arg(long, env = "DOTFILES_TART_VM_NAME")]
    vm_name: Option<String>,
    #[arg(long, env = "DOTFILES_TART_SSH_USER", default_value = "admin")]
    ssh_user: String,
    #[arg(long, env = "DOTFILES_TART_SSH_PASSWORD", default_value = "admin")]
    ssh_password: String,
    #[arg(long, env = "DOTFILES_TART_KEEP_VM", hide = true)]
    keep_vm: Option<String>,
    #[arg(long, env = "DOTFILES_TART_DISK_SIZE_GB", default_value_t = 120)]
    disk_size_gb: u16,
}

#[derive(Clone, Copy, ValueEnum)]
/// ゲストに渡すシナリオ。現状は初期設定から switch までの full のみ。
enum RuntimeScenario {
    Full,
}

/// ホスト準備またはゲスト実行の失敗を、xtask へ非 0 終了として返す。
fn main() -> std::process::ExitCode {
    match TartRunner::new(Args::parse()).and_then(TartRunner::run) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            std::process::ExitCode::FAILURE
        }
    }
}

impl Args {
    /// CLI で省略された場合は full シナリオを選び、文字列による分岐を呼び出し側へ漏らさない。
    fn scenario(&self) -> RuntimeScenario {
        self.scenario.unwrap_or(RuntimeScenario::Full)
    }
}

/// Tart VM と SSH 制御接続の前提値をまとめて所有する。
struct TartRunner {
    scenario: RuntimeScenario,
    source_hash: String,
    image: String,
    vm_name: String,
    temp_dir: PathBuf,
    ssh_user: String,
    ssh_password: String,
    ssh_control_path: String,
    keep_vm: bool,
    disk_size_gb: u16,
    vm_created: bool,
    vm_started: bool,
    tart_child: Option<Child>,
}

impl TartRunner {
    /// macOS/Apple Silicon と必要コマンドを確認し、VM 名と一時ディレクトリを確定する。
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
        let source_hash = args.source_hash.trim().to_string();
        // ゲストはこの commit を GitHub から取得する。空文字は checkout も flake 参照も成立しない。
        if source_hash.is_empty() {
            bail!("source hash must not be empty");
        }

        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let vm_name = args
            .vm_name
            .unwrap_or_else(|| format!("dotfiles-full-{timestamp}"));
        let temp_dir =
            env::temp_dir().join(format!("dotfiles-tart-{timestamp}-{}", std::process::id()));

        Ok(Self {
            scenario,
            source_hash,
            image: args.image,
            vm_name,
            temp_dir,
            ssh_user: args.ssh_user,
            ssh_password: args.ssh_password,
            ssh_control_path: random_ssh_control_path()?,
            keep_vm: args.keep_vm.is_some(),
            disk_size_gb: args.disk_size_gb,
            vm_created: false,
            vm_started: false,
            tart_child: None,
        })
    }

    /// VM を起動し、CI の runner と同じ手順でゲストを整えてからシナリオを起動する。
    fn run(mut self) -> Result<()> {
        fs::create_dir_all(&self.temp_dir)?;

        step("tart clone");
        println!("cloning VM: {} -> {}", self.image, self.vm_name);
        run_plain("tart", ["clone", &self.image, &self.vm_name])?;
        self.vm_created = true;

        step("tart disk resize");
        println!(
            "resizing VM disk: {} -> {}GB",
            self.vm_name, self.disk_size_gb
        );
        let disk_size = self.disk_size_gb.to_string();
        run_plain(
            "tart",
            ["set", &self.vm_name, "--disk-size", disk_size.as_str()],
        )?;

        step("tart run");
        println!("starting VM: {}", self.vm_name);
        let tart_log = File::create(self.temp_dir.join("tart-run.log"))?;
        let tart_log_err = tart_log.try_clone()?;
        let child = Command::new("tart")
            .args(["run", "--no-graphics", &self.vm_name])
            .stdout(Stdio::from(tart_log))
            .stderr(Stdio::from(tart_log_err))
            .spawn()?;
        self.vm_started = true;
        self.tart_child = Some(child);

        step("ssh wait");
        let session = self.connect_ssh()?;
        println!("ssh ready: {}", session.destination());

        step("checkout");
        session.script(&format!(
            "rm -rf {GUEST_REPO_DIR}; git clone {REPO_URL} {GUEST_REPO_DIR}; \
             git -C {GUEST_REPO_DIR} checkout {}",
            self.source_hash
        ))?;

        step("Rust toolchain");
        session.script(INSTALL_RUST_TOOLCHAIN)?;

        step("reset Homebrew installation");
        session.script(RESET_HOMEBREW)?;

        step("runtime integration");
        match self.scenario {
            RuntimeScenario::Full => session.script(&format!(
                "cd {GUEST_REPO_DIR}; export PATH=$HOME/.cargo/bin:$PATH; \
                 cargo run --package dotfiles-integration-test-guest -- --source-hash {}",
                self.source_hash
            )),
        }
    }

    /// Tart が IP を返し、SSH ポートが開くまで待つ。VM プロセスが先に落ちた場合は失敗にする。
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

    /// パスワード認証を 1 回だけ行う SSH 制御マスターを確立し、以後の remote command で共有する。
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

    /// sshpass で制御マスターを起動し、以後は control socket 経由で接続する。
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

    /// macOS の Unix domain socket 長制限に当たらない短い制御ソケットパスを返す。
    fn ssh_control_path(&self) -> String {
        self.ssh_control_path.clone()
    }
}

/// ControlPath の長さ制限を避けるため、短いランダム名を `/tmp` 直下に作る。
fn random_ssh_control_path() -> Result<String> {
    let mut bytes = [0_u8; 4];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(format!("/tmp/df-{:08x}.s", u32::from_ne_bytes(bytes)))
}

/// CI ログでホスト側の進行位置を追えるよう、手順の境界を固定形式で出す。
fn step(label: &str) {
    println!("==> {label}");
}

/// 確立済み control socket と接続先を保持し、remote command を同じ SSH セッションで実行する。
struct SshSession {
    user: String,
    control_path: String,
    master: Child,
    ip: String,
}

impl SshSession {
    /// control socket が作られ、`true` が実行できるまで待つ。パスワード再入力のリトライはしない。
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

    /// 複数コマンドからなるゲスト手順を、remote 側 shell に 1 度だけ解釈させて実行する。
    ///
    /// ssh は remote 引数を空白で連結して remote shell へ渡すため、script 本体は single quote で
    /// 括った 1 語として送る。single quote を含む本体はこの括りを破るので受け付けない。
    fn script(&self, body: &str) -> Result<()> {
        if body.contains('\'') {
            bail!("guest script must not contain single quotes: {body}");
        }
        let command = format!("/bin/bash -c 'set -eux; {body}'");
        self.run(&[command.as_str()])
    }

    /// remote command の非 0 終了を、そのままシナリオ失敗として返す。
    fn run(&self, remote_args: &[&str]) -> Result<()> {
        let status = self.status(remote_args)?;
        if status.success() {
            Ok(())
        } else {
            bail!("ssh command failed: {status}")
        }
    }

    /// control socket がない状態での実行を拒否し、未接続状態を型ではなく状態確認で検出する。
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

    /// user と VM の IP から ssh の接続先文字列を作る。
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

/// Tart などホスト側コマンドをログ表示と同じ引数配列で実行する。
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

/// `tart ip` のように stdout が必要なホスト側コマンドを実行し、失敗時は stderr も出す。
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

/// SSH daemon の準備完了を、認証前の TCP 接続可否で軽く確認する。
fn ssh_port_ready(ip: &str) -> Result<bool> {
    Ok(Command::new("nc")
        .args(["-G", "1", "-z", ip, "22"])
        .status()?
        .success())
}

/// 構築済み `Command` を実行し、失敗時はログに出した label と終了状態を報告する。
fn run_command(mut command: Command, label: &str) -> Result<()> {
    println!("$ {label}");
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        bail!("command failed: {label}: {status}")
    }
}

/// 初回 SSH 制御マスター作成時だけ、環境変数 `SSHPASS` 経由でパスワードを渡す。
fn password_command(program: &str, password: &str) -> Command {
    let mut command = Command::new("sshpass");
    command.args(["-e", program]).env("SSHPASS", password);
    command
}

/// 初回接続では公開鍵認証を無効化し、パスワードプロンプト 1 回だけに固定する。
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

/// 制御マスター確立後は認証方式を追加せず、既存 control socket だけを使う。
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
