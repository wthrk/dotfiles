//! ゲスト側で実行する実行時統合シナリオ。
//!
//! 全体シナリオは 1 つの順序付き手順として扱う。初期状態システムの初期設定、1 人目ユーザーの
//! nix-darwin 切り替え、2 人目ユーザーの導入と更新、管理対象リンクの確認までを一続きで検証する。
//!
//! 1 人目 `ya` と 2 人目 `dotfilesci` は、どちらも適用範囲を指定しない同じ bootstrap 呼び出しを
//! 通り、入る層だけがマシンの状態で変わる。2 人目の手順が 1 人目の後に来るのは、system 層の
//! 所有者が居るマシンでの導入と更新を見るためである。渡す `--force` は、前回実行の生成 flake が
//! 残るゲストでも再実行できるようにするためで、両ユーザーに共通する。
//!
//! 先頭の初期設定手順だけは runner アカウントで `--no-switch` を渡す。この手順が見るのは Nix
//! 未導入状態からインストーラ経路が通ることと、適用しない指定が system 側のパスを触らないことで
//! あり、利用者の導入手順ではない。ここで適用まで走らせると runner が system 層の所有者になり、
//! 1 人目の導入を検証できなくなる。

use std::path::Path;

use crate::{
    Result,
    assertions::{
        assert_managed_links, assert_system_profile_users, ensure_absent_path, ensure_exists,
        ensure_nonempty_path,
    },
    command::{run_with_env, status_with_env, sudo_user_args},
    runtime_env::{
        ScenarioEnv, current_host, current_user, dotfilesci_env, local_config_dir_for_user,
        local_config_flake_for_current_user, local_config_flake_for_user, local_config_ref,
        root_env, user_home, ya_env,
    },
    users::{ensure_local_user, grant_noninteractive_sudo},
};
use anyhow::{Context, bail};
use clap::ValueEnum;
use dotfiles_core::path::{display as path_str, find_executable};

const NIX: &str = "/nix/var/nix/profiles/default/bin/nix";

const FULL_SCENARIO: &[RuntimeStep] = &[
    RuntimeStep::FreshBootstrap,
    RuntimeStep::DarwinSwitchYa,
    RuntimeStep::SecondUserHomeManager,
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
    DarwinSwitchYa,
    SecondUserHomeManager,
}

impl RuntimeStep {
    /// CI ログでどの段階が失敗したか追えるよう、手順ごとの固定ラベルを返す。
    fn label(self) -> &'static str {
        match self {
            RuntimeStep::FreshBootstrap => "fresh-bootstrap",
            RuntimeStep::DarwinSwitchYa => "darwin-switch-ya",
            RuntimeStep::SecondUserHomeManager => "second-user-home-manager",
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
            RuntimeStep::DarwinSwitchYa => self.darwin_switch_ya(),
            RuntimeStep::SecondUserHomeManager => self.second_user_home_manager(),
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

        // runner アカウントはインストーラ経路の検証だけに使う。生成 flake を残すと、後段の
        // 全ユーザー走査がこのアカウントも更新対象にし、runner のホーム内容に結果が左右される。
        self.run(
            "rm",
            &[
                "-rf",
                path_str(local_config_dir_for_user(current_user().as_str())?).as_str(),
            ],
        )
    }

    /// 1 人目 `ya` が、適用範囲を指定しない bootstrap だけで system 層まで導入できることを確認する。
    ///
    /// 誰も system 層を持たないマシンなので、bootstrap は sudo を要求し、CLI は Home Manager に
    /// 続いて nix-darwin まで適用する。同じ呼び出しをもう一度通し、所有者が自分であるマシンでの
    /// 再実行も同じ手順で済むことを見る。
    fn darwin_switch_ya(&self) -> Result<()> {
        self.ensure_nonempty("flake.lock")?;
        self.runner_info()?;
        self.require_existing_nix()?;

        ensure_local_user(self, "ya", "ya", "Ya-Temp-2026!", true)?;
        // bootstrap の sudo 要求も nix-darwin 適用も `ya` 自身の sudo を通る。端末が無い実行環境で
        // その経路を通すため、ゲスト内だけの非対話 sudo をここで与える。
        grant_noninteractive_sudo(self, "ya")?;
        let ya_home = user_home("ya")?;
        if Path::new("/opt/homebrew").is_dir() {
            self.run("sudo", &["chown", "-R", "ya:admin", "/opt/homebrew"])?;
        }

        // `sudo darwin-rebuild` は root が checkout を flake として評価する。root が使うのと同じ
        // HOME で safe.directory を登録し、git の所有者検査で評価が止まらないようにする。
        self.run_as_root(
            "/usr/bin/git",
            &[
                "config",
                "--global",
                "--add",
                "safe.directory",
                self.workspace_str().as_str(),
            ],
        )?;
        // 2 人目と同じ呼び出し。`--user` も `--host` も `--no-switch` も渡さず、適用範囲は
        // マシンの状態から決まるものに任せる。
        self.run_bootstrap_sudo_user("ya", &ya_env(&self.env.nix_config)?, &["--force"])?;

        // system 層は `ya` だけを管理対象にしている必要がある。この結び付きは CLI の scope 判定が
        // 依存する事実そのものであり、home-manager 側の実装が変われば崩れる。
        assert_system_profile_users(&["ya"])?;
        // 所有者が自分であるマシンでの再実行。bootstrap は同じ引数のまま sudo を要求し、適用範囲も
        // 変わらない。ここで別のユーザーのエントリが増えるなら scope 判定が壊れている。
        self.run_bootstrap_sudo_user("ya", &ya_env(&self.env.nix_config)?, &["--force"])?;
        assert_system_profile_users(&["ya"])?;

        let ya_config_dir = path_str(local_config_dir_for_user("ya")?);
        // nix-darwin 切り替え後も生成ローカル設定が評価できる必要がある。
        self.run_as_ya(
            NIX,
            &["flake", "check", "--no-update-lock-file", &ya_config_dir],
        )?;
        let ya_home_activation =
            local_config_ref("ya", "homeConfigurations.ya.activationPackage.drvPath")?;
        self.run_as_ya(
            NIX,
            &["eval", "--no-update-lock-file", ya_home_activation.as_str()],
        )?;
        // 1 人目の生成 flake は nix-darwin の適用先を持つ。出力名はホスト名で、`--host` を渡して
        // いないので CLI が解決したものと同じ値になる。
        let ya_darwin_system = local_config_ref(
            "ya",
            &format!("darwinConfigurations.{}.system", current_host()?),
        )?;
        self.run_as_ya(
            NIX,
            &["eval", "--no-update-lock-file", ya_darwin_system.as_str()],
        )?;
        // 最後の利用者向け検査として、切り替え対象ユーザーが Nix store 由来の
        // 管理対象シェルやエディタ設定ファイルを受け取ったことを確認する。
        assert_managed_links(
            path_str(&ya_home).as_str(),
            &[".config/zsh", ".config/nvim/lua", ".zshrc", ".zshenv"],
        )?;

        Ok(())
    }

    /// 2 人目のユーザーが、1 人目と同じ引数なし bootstrap で home 層だけを導入・更新できることを確認する。
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

        let dotfilesci_env = dotfilesci_env(&self.env.nix_config)?;
        // 手順はユーザーの種類で分かれない。`--user` も `--host` も渡さず、1 人目と同じ形で実行する。
        self.run_bootstrap_sudo_user("dotfilesci", &dotfilesci_env, &["--force"])?;

        // 2 人目のユーザーは管理者ユーザーのホーム状態を流用せず、
        // 自分の生成設定とプロファイルを持つ必要がある。
        ensure_nonempty_path(local_config_flake_for_user("dotfilesci")?)?;
        ensure_exists(dotfilesci_home.join(".nix-profile"))?;
        // Home Manager のアクティベーションは、2 人目ユーザーのホームに期待する管理リンクを
        // 導入する必要がある。
        assert_managed_links(
            path_str(&dotfilesci_home).as_str(),
            &[".config/zsh", ".config/nvim/lua", ".zshrc", ".zshenv"],
        )?;
        let dotfilesci_activation = local_config_ref(
            "dotfilesci",
            "homeConfigurations.dotfilesci.activationPackage.drvPath",
        )?;
        // ローカル設定の所有者と同じユーザーでアクティベーションパスを評価する。
        // これにより権限と出力名の退行を検出する。
        self.run_sudo_user(
            "dotfilesci",
            &dotfilesci_env,
            NIX,
            &[
                "eval",
                "--no-update-lock-file",
                dotfilesci_activation.as_str(),
            ],
        )?;
        // system 層を別ユーザーが持つマシンでは、生成 flake に nix-darwin の適用先が無い。
        // 生成ファイルの本文ではなく、評価が成立しないことで確認する。
        let dotfilesci_darwin = local_config_ref(
            "dotfilesci",
            &format!("darwinConfigurations.{}.system", current_host()?),
        )?;
        if self
            .status_sudo_user(
                "dotfilesci",
                &dotfilesci_env,
                NIX,
                &["eval", "--no-update-lock-file", dotfilesci_darwin.as_str()],
            )?
            .success()
        {
            bail!("2 人目の生成 flake が darwinConfigurations を持っている: {dotfilesci_darwin}");
        }

        // 2 人目が引数なし `dotfiles update` を実行しても、system 層の所有者は移らない。
        self.run_sudo_user(
            "dotfilesci",
            &dotfilesci_env,
            NIX,
            &["run", self.dotfiles_source_str(), "--", "update"],
        )?;
        assert_system_profile_users(&["ya"])?;

        // 走査が届いたユーザーだけが復旧する状態を先に作る。両ユーザーのホームから管理リンクを
        // 1 つずつ外し、走査後にそれが戻っていることを見る。これが無いと、走査が 0 人でも所有者
        // 1 人でも後続の確認は成立したままになる。
        let ya_home = user_home("ya")?;
        self.remove_managed_link(&dotfilesci_home, ".config/zsh")?;
        self.remove_managed_link(&ya_home, ".config/zsh")?;

        // auto-update daemon と同じ root からの全ユーザー走査。両ユーザーが更新され、system 層は
        // 所有者 `ya` の flake からだけ適用される。
        self.run_as_root(NIX, &["run", self.dotfiles_source_str(), "--", "update"])?;
        assert_system_profile_users(&["ya"])?;
        assert_managed_links(
            path_str(&dotfilesci_home).as_str(),
            &[".config/zsh", ".config/nvim/lua", ".zshrc", ".zshenv"],
        )?;
        assert_managed_links(
            path_str(&ya_home).as_str(),
            &[".config/zsh", ".config/nvim/lua", ".zshrc", ".zshenv"],
        )
    }

    /// 全ユーザー走査の対象になったかを観測するため、指定ユーザーのホームから管理リンクを外す。
    ///
    /// Home Manager のアクティベーションは生成が変わらなくてもリンクを張り直すので、外したリンクは
    /// そのユーザーが更新された場合にだけ戻る。`-f` を付けないのは、外す対象が既に無い状態で復旧の
    /// 確認が空振りになるのを避けるためである。
    fn remove_managed_link(&self, home: &Path, relative: &str) -> Result<()> {
        self.run("sudo", &["rm", path_str(home.join(relative)).as_str()])
    }

    /// 失敗時の環境差分を追えるよう、OS、kernel、ユーザー、Xcode path をログに出す。
    fn runner_info(&self) -> Result<()> {
        self.run("sw_vers", &[])?;
        self.run("uname", &["-a"])?;
        self.run("id", &[])?;
        self.run("xcode-select", &["-p"])
    }

    /// 現在ユーザーで bootstrap の no-switch 経路を実行し、ローカル flake 生成だけを確認する。
    ///
    /// 利用者名もホスト名も渡さない。この手順が確かめるのはインストーラ経路と「適用しない」指定で
    /// あって、生成 flake に書かれる名前ではない。
    fn bootstrap_current_user_no_switch(&self) -> Result<()> {
        self.run_bootstrap(&["--no-switch", "--force"])
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
        self.env
            .pass_source_to_bootstrap
            .then(|| {
                [
                    "--source".to_string(),
                    self.dotfiles_source_str().to_string(),
                ]
            })
            .into_iter()
            .flatten()
            .chain(args.iter().map(|arg| (*arg).to_string()))
            .collect()
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
        let envs = envs
            .iter()
            .cloned()
            .chain(
                self.env
                    .bootstrap_source_ref_env
                    .as_ref()
                    .map(|source_ref| {
                        (
                            "DOTFILES_BOOTSTRAP_SOURCE_REF".to_string(),
                            source_ref.clone(),
                        )
                    }),
            )
            .collect::<Vec<_>>();
        run_with_env(
            Some(&self.env),
            "sudo",
            sudo_user_args(user, &envs, program, args),
        )
    }

    /// 別ユーザーで実行し、失敗も観測対象になる確認のために終了状態だけを返す。
    fn status_sudo_user(
        &self,
        user: &str,
        envs: &[(String, String)],
        program: &str,
        args: &[&str],
    ) -> Result<std::process::ExitStatus> {
        status_with_env(
            Some(&self.env),
            "sudo",
            sudo_user_args(user, envs, program, args),
        )
    }

    /// auto-update daemon と同じ root 環境で実行する。対象ユーザーは渡さない。
    fn run_as_root(&self, program: &str, args: &[&str]) -> Result<()> {
        self.run_sudo_user("root", &root_env(&self.env.nix_config)?, program, args)
    }

    /// `ya` 用のログイン風環境を使って、Darwin switch 後の評価と確認を行う。
    fn run_as_ya(&self, program: &str, args: &[&str]) -> Result<()> {
        self.run_sudo_user("ya", &ya_env(&self.env.nix_config)?, program, args)
    }
}
