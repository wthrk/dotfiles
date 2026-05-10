//! VM を使わずに実行できる静的検証。
//!
//! Rust ワークスペース、bootstrap shell script、Nix flake、公開 Nix module の評価をここで扱う。

use std::{env, fs, path::PathBuf, process};

use xshell::{Shell, cmd};

use crate::{Result, command::step};

/// dirty な実マシン状態に依存しない、リポジトリ内だけで完結する検証を実行する。
pub(crate) fn check() -> Result<()> {
    let shell = Shell::new()?;
    rust(&shell)?;
    shell_scripts(&shell)?;
    nix(&shell)?;
    nix_diagnostics(&shell)?;
    runner_home(&shell)?;
    exported_modules(&shell)
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

/// bootstrap 用 shell script の構文を検証する。
fn shell_scripts(shell: &Shell) -> Result<()> {
    step("shell scripts");
    cmd!(shell, "bash -n scripts/bootstrap.sh").run()?;
    Ok(())
}

/// lock file が存在する状態で、Nix flake の評価と Nix ファイルの整形を検証する。
fn nix(shell: &Shell) -> Result<()> {
    step("flake.lock exists");
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

    step("nil diagnostics");
    cmd!(shell, "nil diagnostics --deny-warnings {files...}").run()?;
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
    cmd!(
        shell,
        "nix eval --no-update-lock-file {config_dir_path}#homeConfigurations.runner.activationPackage.drvPath"
    )
    .run()?;

    step("exported nix-darwin module eval");
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
