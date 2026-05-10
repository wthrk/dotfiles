//! `cargo xtask check` から呼ばれる検証本体。
//!
//! xtask は起動コマンドだけを持つため、Rust、Nix、公開モジュール、zsh 挙動の検証手順はここへ集約する。
//! runtime 統合検証だけは Tart ゲストを使う別クレートへ委譲する。

use std::{env, fs, path::PathBuf, process};

use anyhow::bail;
use clap::{Parser, Subcommand, ValueEnum};
use xshell::{Shell, cmd};

mod command;
mod zsh;

use command::step;

type Result<T> = dotfiles_core::Result<T>;

#[derive(Parser)]
#[command(name = "dotfiles-checks")]
/// `cargo xtask check` から渡される検証グループ。
struct Cli {
    #[command(subcommand)]
    target: Option<CheckTarget>,
}

#[derive(Subcommand)]
/// VM なしで実行できる検証と、VM が必要な統合検証を分ける。
enum CheckTarget {
    Static,
    Zsh,
    Integration {
        #[arg(value_enum)]
        scenario: Option<RuntimeScenario>,
        #[arg(long, env = "DOTFILES_TEST_SOURCE_HASH")]
        source_hash: Option<String>,
    },
    All,
}

#[derive(Clone, Copy, ValueEnum)]
/// 統合テスト実行器へ渡すシナリオ。現状は初期設定から switch までの full のみ。
enum RuntimeScenario {
    Full,
}

/// anyhow の失敗を標準エラーへ出し、xtask へ非 0 終了として返す。
fn main() -> std::process::ExitCode {
    match run(Cli::parse().target) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// 未指定時はローカル開発で毎回回す検証に絞り、VM 統合検証は明示指定時だけ動かす。
fn run(target: Option<CheckTarget>) -> Result<()> {
    match target {
        None => default_checks(),
        Some(CheckTarget::Static) => static_checks(),
        Some(CheckTarget::Zsh) => zsh::check(),
        Some(CheckTarget::All) => all_checks(),
        Some(CheckTarget::Integration {
            scenario,
            source_hash,
        }) => integration(scenario.unwrap_or(RuntimeScenario::Full), source_hash),
    }
}

/// dirty な実マシン状態に依存しない、リポジトリ内だけで完結する検証を実行する。
fn static_checks() -> Result<()> {
    let shell = Shell::new()?;
    rust(&shell)?;
    nix(&shell)?;
    nix_diagnostics(&shell)?;
    runner_home(&shell)?;
    exported_modules(&shell)
}

/// 開発時の既定検証として、静的検証に加えて生成 zsh 設定の起動確認も行う。
fn default_checks() -> Result<()> {
    static_checks()?;
    zsh::check()
}

/// VM 内での初期導入シナリオまで含めて実行する。
fn all_checks() -> Result<()> {
    default_checks()?;
    integration(RuntimeScenario::Full, None)
}

/// Rust ワークスペース全体で、警告を失敗扱いにして整形、型検査、lint、テストを回す。
fn rust(shell: &Shell) -> Result<()> {
    step("cargo fmt");
    cmd!(shell, "cargo fmt --all -- --check").run()?;
    step("cargo check");
    cmd!(shell, "env RUSTFLAGS='-D warnings' cargo check --workspace").run()?;
    step("cargo clippy");
    cmd!(
        shell,
        "cargo clippy --workspace --all-targets -- -D warnings"
    )
    .run()?;
    step("cargo test");
    cmd!(
        shell,
        "env RUSTFLAGS='-D warnings' cargo test --workspace --all-targets"
    )
    .run()?;
    Ok(())
}

/// lock file が存在する状態で、Nix flake の評価と Nix ファイルの整形を検証する。
fn nix(shell: &Shell) -> Result<()> {
    step("flake.lock exists");
    // リポジトリには明示的なロックファイルが必要。検証が暗黙の flake 入力解決に
    // 依存していないことをここで確認する。
    cmd!(shell, "test -s flake.lock").run()?;
    step("nix flake check");
    cmd!(shell, "nix flake check --no-update-lock-file").run()?;
    let files = nix_files(shell)?;
    if !files.is_empty() {
        step("nix fmt");
        cmd!(shell, "nix fmt -- --ci {files...}").run()?;
    }
    Ok(())
}

/// devShell に入っている `nil` で Nix 診断を実行し、モジュール評価の静的な崩れを検出する。
fn nix_diagnostics(shell: &Shell) -> Result<()> {
    let files = nix_files(shell)?;
    if files.is_empty() {
        return Ok(());
    }
    let nil = cmd!(shell, "command -v nil").ignore_status().read()?;
    if nil.trim().is_empty() {
        step("nil diagnostics skipped (nil not found)");
        return Ok(());
    }

    step("nil diagnostics");
    let mut diagnostics = Vec::new();
    for file in files {
        let output = cmd!(shell, "nil diagnostics {file}").read()?;
        if !output.trim().is_empty() {
            diagnostics.push(format!("{file}:\n{output}"));
        }
    }
    if !diagnostics.is_empty() {
        // Nix モジュールは生成 flake 向けの公開 API なので、
        // nil 診断は実際の失敗として扱う。
        bail!(
            "nix diagnostics reported issues:\n{}",
            diagnostics.join("\n")
        );
    }
    Ok(())
}

/// `dotfiles init` が作ったローカル flake から Home Manager 出力を評価できることを確認する。
fn runner_home(shell: &Shell) -> Result<()> {
    let config_dir = TempDir::new("dotfiles-check")?;
    let config_dir_path = config_dir.path().display().to_string();
    let source = env::current_dir()?.canonicalize()?.display().to_string();

    step("dotfiles init output");
    cmd!(
        shell,
        "env DOTFILES_CONFIG_DIR={config_dir_path} cargo run --package dotfiles-cli -- init --user runner --host runner --system aarch64-darwin --source {source}"
    )
    .run()?;

    step("runner Home Manager output eval");
    cmd!(
        shell,
        "nix eval --no-update-lock-file {config_dir_path}#homeConfigurations.runner.activationPackage.drvPath"
    )
    .run()?;
    Ok(())
}

/// `homeManagerModules.default` と `darwinModules.default` が外部 flake から単独で評価できることを確認する。
fn exported_modules(shell: &Shell) -> Result<()> {
    let config_dir = TempDir::new("dotfiles-module-check")?;
    let config_dir_path = config_dir.path().display().to_string();
    let source = env::current_dir()?.canonicalize()?.display().to_string();
    fs::write(
        config_dir.path().join("flake.nix"),
        external_module_flake(&source),
    )?;

    step("exported module flake lock");
    cmd!(shell, "nix flake lock {config_dir_path}").run()?;

    step("exported Home Manager module eval");
    // `lib.mkHome` だけではなく、公開モジュールの `homeManagerModules.default` 経由で
    // 動作する必要がある。
    cmd!(
        shell,
        "nix eval --no-update-lock-file {config_dir_path}#homeConfigurations.runner.activationPackage.drvPath"
    )
    .run()?;

    step("exported nix-darwin module eval");
    // `darwinModules.default` に隠れた specialArgs 依存が混入した場合に検出する。
    cmd!(
        shell,
        "nix eval --no-update-lock-file {config_dir_path}#darwinConfigurations.runner.system"
    )
    .run()?;
    Ok(())
}

/// 公開モジュールを利用側 flake が直接読み込むときの最小構成を生成する。
fn external_module_flake(source: &str) -> String {
    format!(
        r#"{{
  inputs = {{
    dotfiles.url = "path:{source}";
    nixpkgs.follows = "dotfiles/nixpkgs";
    home-manager.follows = "dotfiles/home-manager";
    darwin.follows = "dotfiles/darwin";
  }};

  outputs = {{ dotfiles, nixpkgs, home-manager, darwin, ... }}:
    let
      system = "aarch64-darwin";
      pkgs = import nixpkgs {{ inherit system; config.allowUnfree = true; }};
    in {{
      homeConfigurations.runner = home-manager.lib.homeManagerConfiguration {{
        inherit pkgs;
        modules = [
          dotfiles.homeManagerModules.default
          {{ dotfiles.user = "runner"; }}
        ];
      }};

      darwinConfigurations.runner = darwin.lib.darwinSystem {{
        inherit system;
        modules = [
          dotfiles.darwinModules.default
          {{
            dotfiles = {{
              user = "runner";
              host = "runner";
            }};
          }}
        ];
      }};
    }};
}}
"#,
        source = source
    )
}

/// VM の準備と guest 実行は integration クレート側へ任せる。
fn integration(scenario: RuntimeScenario, source_hash: Option<String>) -> Result<()> {
    let shell = Shell::new()?;
    match scenario {
        RuntimeScenario::Full => {
            let mut args = vec![
                "run".to_string(),
                "--package".to_string(),
                "dotfiles-integration-tests".to_string(),
            ];
            if let Some(source_hash) = source_hash {
                args.extend(["--".to_string(), "--source-hash".to_string(), source_hash]);
            }
            cmd!(shell, "cargo {args...}").run()?;
        }
    }
    Ok(())
}

/// `target` 配下を除外し、整形と nil 診断の対象になる Nix ファイルだけを列挙する。
fn nix_files(shell: &Shell) -> Result<Vec<String>> {
    Ok(cmd!(
        shell,
        "find . -path ./target -prune -o -name '*.nix' -type f -print"
    )
    .read()?
    .lines()
    .map(|path| path.trim_start_matches("./"))
    .map(ToOwned::to_owned)
    .collect())
}

/// 生成 flake を置く検証用ディレクトリを、検証終了時に消すための所有者。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// 同じプロセス ID の残骸を先に消し、検証対象が前回の flake.lock を読まないようにする。
    fn new(prefix: &str) -> Result<Self> {
        let path = env::temp_dir().join(format!("{prefix}-{}", process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    /// xshell の command interpolation に渡すため、所有中のパスを参照で返す。
    fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
