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
pub(crate) enum RuntimeScenario {
    Full,
}

#[derive(Clone, Copy)]
enum RuntimeStep {
    FreshBootstrap,
    SecondUserHomeManager,
    DarwinSwitchYa,
}

impl RuntimeStep {
    fn label(self) -> &'static str {
        match self {
            RuntimeStep::FreshBootstrap => "fresh-bootstrap",
            RuntimeStep::SecondUserHomeManager => "second-user-home-manager",
            RuntimeStep::DarwinSwitchYa => "darwin-switch-ya",
        }
    }
}

pub(crate) struct ScenarioRunner {
    pub(crate) env: ScenarioEnv,
}

impl ScenarioRunner {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            env: ScenarioEnv::current()?,
        })
    }

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

    pub(crate) fn run(&self, program: &str, args: &[&str]) -> Result<()> {
        run_with_env(Some(&self.env), program, args)
    }

    pub(crate) fn status(&self, program: &str, args: &[&str]) -> Result<std::process::ExitStatus> {
        status_with_env(Some(&self.env), program, args)
    }

    fn run_step(&self, step: RuntimeStep) -> Result<()> {
        match step {
            RuntimeStep::FreshBootstrap => self.fresh_bootstrap(),
            RuntimeStep::SecondUserHomeManager => self.second_user_home_manager(),
            RuntimeStep::DarwinSwitchYa => self.darwin_switch_ya(),
        }
    }

    fn fresh_bootstrap(&self) -> Result<()> {
        self.ensure_nonempty("flake.lock")?;
        self.runner_info()?;

        if let Some(nix) = find_executable("nix") {
            bail!(
                "ゼロ状態の導入テストでは Nix 未導入を前提にします: {}",
                nix.display()
            );
        }

        self.run("scripts/bootstrap.sh", &["--dry-run"])?;
        self.bootstrap_current_user_no_switch()?;
        ensure_nonempty_path(local_config_flake_for_current_user()?)?;

        self.ensure_absent("/etc/bashrc.before-nix-darwin")?;
        self.ensure_absent("/etc/zshrc.before-nix-darwin")?;
        self.ensure_absent("/opt/homebrew/Library/Taps.before-nix-homebrew")?;
        self.ensure_absent("/usr/local/Library/Taps.before-nix-homebrew")?;

        self.run(
            path_str(self.env.workspace.join("scripts/bootstrap.sh")).as_str(),
            &[
                "--source",
                self.workspace_str().as_str(),
                "--user",
                current_user().as_str(),
                "--host",
                current_host()?.as_str(),
                "--mode",
                "darwin",
                "--no-switch",
                "--force",
            ],
        )?;

        self.run("scripts/bootstrap.sh", &["--self-test"])
    }

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

        self.run_sudo_user(
            "dotfilesci",
            &dotfilesci_env(&self.env.nix_config)?,
            path_str(self.env.workspace.join("scripts/bootstrap.sh")).as_str(),
            &[
                "--source",
                self.workspace_str().as_str(),
                "--user",
                "dotfilesci",
                "--host",
                "dotfilesci",
                "--mode",
                "home-manager",
                "--run-switch",
                "--force",
            ],
        )?;

        ensure_nonempty_path(local_config_flake_for_user("dotfilesci")?)?;
        ensure_exists(dotfilesci_home.join(".nix-profile"))?;
        assert_managed_links(
            path_str(&dotfilesci_home).as_str(),
            &[".config/zsh", ".config/nvim", ".zshrc", ".zshenv"],
        )?;
        let dotfilesci_activation = local_config_ref(
            "dotfilesci",
            "homeConfigurations.dotfilesci.activationPackage.drvPath",
        )?;
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
        self.run_sudo_user(
            "ya",
            &ya_env(&self.env.nix_config)?,
            path_str(self.env.workspace.join("scripts/bootstrap.sh")).as_str(),
            &[
                "--source",
                self.workspace_str().as_str(),
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
                self.workspace_str().as_str(),
                "--",
                "switch",
                "darwin",
                "--config-dir",
                &ya_config_dir,
                "--host",
                "ya",
            ],
        )?;
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
        assert_managed_links(
            path_str(&ya_home).as_str(),
            &[".config/zsh", ".config/nvim", ".zshrc", ".zshenv"],
        )?;

        Ok(())
    }

    fn runner_info(&self) -> Result<()> {
        self.run("sw_vers", &[])?;
        self.run("uname", &["-a"])?;
        self.run("id", &[])?;
        self.run("xcode-select", &["-p"])
    }

    fn bootstrap_current_user_no_switch(&self) -> Result<()> {
        self.run(
            "scripts/bootstrap.sh",
            &[
                "--source",
                self.workspace_str().as_str(),
                "--user",
                current_user().as_str(),
                "--host",
                current_host()?.as_str(),
                "--mode",
                "darwin",
                "--no-switch",
                "--force",
            ],
        )
    }

    fn ensure_nonempty(&self, path: &str) -> Result<()> {
        ensure_nonempty_path(self.env.workspace.join(path))
    }

    fn require_existing_nix(&self) -> Result<()> {
        let nix = Path::new("/nix/var/nix/profiles/default/bin/nix");
        let nix_daemon_profile =
            Path::new("/nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh");
        ensure_nonempty_path(nix).context("second-user-home-manager requires existing Nix")?;
        ensure_nonempty_path(nix_daemon_profile)
            .context("second-user-home-manager requires existing Nix daemon profile")
    }

    fn ensure_absent(&self, path: &str) -> Result<()> {
        ensure_absent_path(path)
    }

    fn workspace_str(&self) -> String {
        path_str(&self.env.workspace)
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
        self.run_sudo_user("ya", &ya_env(&self.env.nix_config)?, program, args)
    }
}
