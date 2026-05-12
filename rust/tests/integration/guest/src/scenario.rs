//! ゲスト側で実行する実行時統合シナリオ。
//!
//! 全体シナリオは 1 つの順序付き手順として扱う。初期状態システムの初期設定、
//! 2 人目ユーザーの Home Manager 切り替え、対象ユーザーの nix-darwin 切り替え、
//! 管理対象リンクの確認までを一続きで検証する。

use std::path::Path;

use crate::{
    Result,
    assertions::{assert_managed_links, ensure_absent_path, ensure_exists, ensure_nonempty_path},
    command::{run_with_env, status_with_env, sudo_user_args},
    runtime_env::{
        ScenarioEnv, current_host, current_user, dotfilesci_env,
        local_config_flake_for_current_user, local_config_flake_for_user, local_config_ref,
        user_home, ya_env,
    },
    users::ensure_local_user,
};
use anyhow::{Context, bail};
use clap::ValueEnum;
use dotfiles_core::path::{display as path_str, find_executable};

const FULL_SCENARIO: &[RuntimeStep] = &[
    RuntimeStep::FreshBootstrap,
    RuntimeStep::SecondUserHomeManager,
    RuntimeStep::DarwinSwitchYa,
];

#[derive(Clone, Copy, ValueEnum)]
/// ホスト側 runner または CI が選択するシナリオ。現状は順序固定の full のみ。
pub(crate) enum RuntimeScenario {
    Full,
}

#[derive(Clone, Copy)]
/// full シナリオ内の順序付き手順。前段の成果物を後段が使うため、独立実行にはしない。
enum RuntimeStep {
    FreshBootstrap,
    SecondUserHomeManager,
    DarwinSwitchYa,
}

impl RuntimeStep {
    /// CI ログでどの段階が失敗したか追えるよう、手順ごとの固定ラベルを返す。
    fn label(self) -> &'static str {
        match self {
            RuntimeStep::FreshBootstrap => "fresh-bootstrap",
            RuntimeStep::SecondUserHomeManager => "second-user-home-manager",
            RuntimeStep::DarwinSwitchYa => "darwin-switch-ya",
        }
    }
}

/// 検出済みのゲスト環境を保持し、各手順を同じ checkout と Nix 設定で実行する。
pub(crate) struct ScenarioRunner {
    pub(crate) env: ScenarioEnv,
}

impl ScenarioRunner {
    /// 現在のゲスト環境を 1 回だけ検出し、以後の手順で共有する。
    pub(crate) fn new(source_hash: Option<String>) -> Result<Self> {
        Ok(Self {
            env: ScenarioEnv::current(source_hash)?,
        })
    }

    /// full シナリオの手順を定義順に実行し、途中失敗したら後続手順へ進まない。
    pub(crate) fn run_scenario(&self, scenario: RuntimeScenario) -> Result<()> {
        match scenario {
            RuntimeScenario::Full => {
                for step in FULL_SCENARIO {
                    println!("==> integration scenario start: {}", step.label());
                    self.run_step(*step)?;
                    println!("==> integration scenario ok: {}", step.label());
                }
                Ok(())
            }
        }
    }

    /// シナリオ共通の作業ディレクトリと環境を反映してコマンドを実行する。
    pub(crate) fn run(&self, program: &str, args: &[&str]) -> Result<()> {
        run_with_env(Some(&self.env), program, args)
    }

    /// `id <user>` のように失敗も期待結果になる確認で、終了状態を呼び出し側に返す。
    pub(crate) fn status(&self, program: &str, args: &[&str]) -> Result<std::process::ExitStatus> {
        status_with_env(Some(&self.env), program, args)
    }

    /// enum の各手順を対応する実装へ写像し、文字列による分岐を避ける。
    fn run_step(&self, step: RuntimeStep) -> Result<()> {
        match step {
            RuntimeStep::FreshBootstrap => self.fresh_bootstrap(),
            RuntimeStep::SecondUserHomeManager => self.second_user_home_manager(),
            RuntimeStep::DarwinSwitchYa => self.darwin_switch_ya(),
        }
    }

    /// Nix 未導入状態から bootstrap がローカル設定を作ることを確認する。
    fn fresh_bootstrap(&self) -> Result<()> {
        self.ensure_nonempty("flake.lock")?;
        self.runner_info()?;

        // 最初の手順はインストーラ経路の検証そのものなので、既に Nix がある場合は
        // 初期状態の経路を検証できていない。
        if let Some(nix) = find_executable("nix") {
            bail!(
                "ゼロ状態の導入テストでは Nix 未導入を前提にします: {}",
                nix.display()
            );
        }

        self.bootstrap_current_user_no_switch()?;
        // 切り替えなしの初期設定でも、利用可能なローカル flake は書かれている必要がある。
        ensure_nonempty_path(local_config_flake_for_current_user()?)?;

        // 切り替えなしの初期設定は、システムパスを変更してはいけない。
        // システムパスの準備は `dotfiles switch darwin` だけが行う。
        self.ensure_absent("/etc/bashrc.before-nix-darwin")?;
        self.ensure_absent("/etc/zshrc.before-nix-darwin")?;
        self.ensure_absent("/opt/homebrew/Library/Taps.before-nix-homebrew")?;
        self.ensure_absent("/usr/local/Library/Taps.before-nix-homebrew")?;

        self.run_bootstrap(&[
            "--user",
            current_user().as_str(),
            "--host",
            current_host()?.as_str(),
            "--mode",
            "darwin",
            "--no-switch",
            "--force",
        ])
    }

    /// 追加ユーザーが自分のローカル flake から Home Manager switch できることを確認する。
    fn second_user_home_manager(&self) -> Result<()> {
        self.ensure_nonempty("flake.lock")?;
        self.runner_info()?;
        self.require_existing_nix()?;

        ensure_local_user(
            self,
            "dotfilesci",
            "Dotfiles CI",
            "DotfilesCI-Temp-2026!",
            false,
        )?;
        let dotfilesci_home = user_home("dotfilesci")?;
        self.run(
            "sudo",
            &[
                "rm",
                "-rf",
                path_str(dotfilesci_home.join(".dotfiles")).as_str(),
            ],
        )?;

        self.run_bootstrap_sudo_user(
            "dotfilesci",
            &dotfilesci_env(&self.env.nix_config)?,
            &[
                "--user",
                "dotfilesci",
                "--host",
                "dotfilesci",
                "--mode",
                "home-manager",
                "--force",
            ],
        )?;

        // 2 人目のユーザーは管理者ユーザーのホーム状態を流用せず、
        // 自分の生成設定とプロファイルを持つ必要がある。
        ensure_nonempty_path(local_config_flake_for_user("dotfilesci")?)?;
        ensure_exists(dotfilesci_home.join(".nix-profile"))?;
        // Home Manager のアクティベーションは、2 人目ユーザーのホームに期待する管理リンクを
        // 導入する必要がある。
        assert_managed_links(
            path_str(&dotfilesci_home).as_str(),
            &[".config/zsh", ".config/nvim", ".zshrc", ".zshenv"],
        )?;
        let dotfilesci_activation = local_config_ref(
            "dotfilesci",
            "homeConfigurations.dotfilesci.activationPackage.drvPath",
        )?;
        // ローカル設定の所有者と同じユーザーでアクティベーションパスを評価する。
        // これにより権限と出力名の退行を検出する。
        self.run_sudo_user(
            "dotfilesci",
            &dotfilesci_env(&self.env.nix_config)?,
            "/nix/var/nix/profiles/default/bin/nix",
            &[
                "eval",
                "--no-update-lock-file",
                dotfilesci_activation.as_str(),
            ],
        )
    }

    /// 対象ユーザー `ya` のローカル flake から nix-darwin switch できることを確認する。
    fn darwin_switch_ya(&self) -> Result<()> {
        self.ensure_nonempty("flake.lock")?;
        self.runner_info()?;

        ensure_local_user(self, "ya", "ya", "Ya-Temp-2026!", true)?;
        let ya_home = user_home("ya")?;
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
        self.run_bootstrap_sudo_user(
            "ya",
            &ya_env(&self.env.nix_config)?,
            &[
                "--user",
                "ya",
                "--host",
                "ya",
                "--mode",
                "darwin",
                "--no-switch",
                "--force",
            ],
        )?;

        let nix = "/nix/var/nix/profiles/default/bin/nix";
        let ya_config_dir = path_str(user_home("ya")?.join(".config/dotfiles"));
        self.run(
            nix,
            &[
                "run",
                self.dotfiles_source_str(),
                "--",
                "switch",
                "darwin",
                "--config-dir",
                &ya_config_dir,
                "--host",
                "ya",
            ],
        )?;
        // nix-darwin 切り替え後も生成ローカル設定が評価できる必要がある。
        self.run_as_ya(
            nix,
            &["flake", "check", "--no-update-lock-file", &ya_config_dir],
        )?;
        let ya_home_activation =
            local_config_ref("ya", "homeConfigurations.ya.activationPackage.drvPath")?;
        self.run_as_ya(
            nix,
            &["eval", "--no-update-lock-file", ya_home_activation.as_str()],
        )?;
        let ya_darwin_system = local_config_ref("ya", "darwinConfigurations.ya.system")?;
        self.run_as_ya(
            nix,
            &["eval", "--no-update-lock-file", ya_darwin_system.as_str()],
        )?;
        // 最後の利用者向け検査として、切り替え対象ユーザーが Nix store 由来の
        // 管理対象シェルやエディタ設定ファイルを受け取ったことを確認する。
        assert_managed_links(
            path_str(&ya_home).as_str(),
            &[".config/zsh", ".config/nvim", ".zshrc", ".zshenv"],
        )?;

        Ok(())
    }

    /// 失敗時の環境差分を追えるよう、OS、kernel、ユーザー、Xcode path をログに出す。
    fn runner_info(&self) -> Result<()> {
        self.run("sw_vers", &[])?;
        self.run("uname", &["-a"])?;
        self.run("id", &[])?;
        self.run("xcode-select", &["-p"])
    }

    /// 現在ユーザーで bootstrap の no-switch 経路を実行し、ローカル flake 生成だけを確認する。
    fn bootstrap_current_user_no_switch(&self) -> Result<()> {
        self.run_bootstrap(&[
            "--user",
            current_user().as_str(),
            "--host",
            current_host()?.as_str(),
            "--mode",
            "darwin",
            "--no-switch",
            "--force",
        ])
    }

    /// checkout 内の前提ファイルが空でないことを確認し、壊れた共有マウントを早めに検出する。
    fn ensure_nonempty(&self, path: &str) -> Result<()> {
        ensure_nonempty_path(self.env.workspace.join(path))
    }

    /// 2 人目以降の手順が、前段で入った multi-user Nix を使っていることを確認する。
    fn require_existing_nix(&self) -> Result<()> {
        let nix = Path::new("/nix/var/nix/profiles/default/bin/nix");
        let nix_daemon_profile =
            Path::new("/nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh");
        ensure_nonempty_path(nix).context("second-user-home-manager requires existing Nix")?;
        ensure_nonempty_path(nix_daemon_profile)
            .context("second-user-home-manager requires existing Nix daemon profile")
    }

    /// dry-run/no-switch がシステム側のパスを作っていないことを検証する。
    fn ensure_absent(&self, path: &str) -> Result<()> {
        ensure_absent_path(path)
    }

    /// guest 内で見えている checkout パスを、bootstrap の `--source` に渡す文字列へ変換する。
    fn workspace_str(&self) -> String {
        path_str(&self.env.workspace)
    }

    fn bootstrap_script_str(&self) -> String {
        path_str(&self.env.bootstrap_script)
    }

    fn dotfiles_source_str(&self) -> &str {
        &self.env.dotfiles_source
    }

    fn bootstrap_args(&self, args: &[&str]) -> Vec<String> {
        let mut result = Vec::new();
        if self.env.pass_source_to_bootstrap {
            result.push("--source".to_string());
            result.push(self.dotfiles_source_str().to_string());
        }
        result.extend(args.iter().map(|arg| (*arg).to_string()));
        result
    }

    fn run_bootstrap(&self, args: &[&str]) -> Result<()> {
        run_with_env(
            Some(&self.env),
            self.bootstrap_script_str().as_str(),
            self.bootstrap_args(args),
        )
    }

    fn run_bootstrap_sudo_user(
        &self,
        user: &str,
        envs: &[(String, String)],
        args: &[&str],
    ) -> Result<()> {
        let program = self.bootstrap_script_str();
        let args = self.bootstrap_args(args);
        let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
        self.run_sudo_user(user, envs, program.as_str(), &arg_refs)
    }

    /// 別ユーザーの HOME/PATH を明示して `sudo -u` し、呼び出し元環境の漏れを防ぐ。
    fn run_sudo_user(
        &self,
        user: &str,
        envs: &[(String, String)],
        program: &str,
        args: &[&str],
    ) -> Result<()> {
        let mut envs = envs.to_vec();
        if let Some(source_ref) = &self.env.bootstrap_source_ref_env {
            envs.push((
                "DOTFILES_BOOTSTRAP_SOURCE_REF".to_string(),
                source_ref.clone(),
            ));
        }
        run_with_env(
            Some(&self.env),
            "sudo",
            sudo_user_args(user, &envs, program, args),
        )
    }

    /// `ya` 用のログイン風環境を使って、Darwin switch 後の評価と確認を行う。
    fn run_as_ya(&self, program: &str, args: &[&str]) -> Result<()> {
        self.run_sudo_user("ya", &ya_env(&self.env.nix_config)?, program, args)
    }
}
