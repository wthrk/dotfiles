//! VM を使わずに実行できる静的検証。
//!
//! Rust、shell script、Nix flake などの外部検証コマンドを順に実行する。

use anyhow::bail;
use xshell::{Shell, cmd};

use crate::{Result, command::step};

/// nightly bump が版差分の算出対象にする参照構成。`nightly-update.yml` の `CI_REFERENCE` と同じ値を指す。
const CI_REFERENCE: &str = "darwinConfigurations.ci-ref";

/// dirty な実マシン状態に依存しない、リポジトリ内だけで完結する検証を実行する。
pub(crate) fn check() -> Result<()> {
    let shell = Shell::new()?;
    rust(&shell)?;
    shell_scripts(&shell)?;
    github_actions(&shell)?;
    nix_diagnostics(&shell)?;
    nix(&shell)
}

/// Rust ワークスペース全体で、警告を失敗扱いにして整形、lint、テストを回す。型検査は lint に内包する。
fn rust(shell: &Shell) -> Result<()> {
    step("cargo fmt");
    cmd!(shell, "cargo fmt --all -- --check").run()?;
    // clippy は `cargo check` の上位互換なので check は走らせない。RUSTFLAGS は cargo の fingerprint に
    // 入るため、後続の `cargo test` と揃えないと依存が pass ごとに再ビルドされる。
    step("cargo clippy");
    cmd!(
        shell,
        "env RUSTFLAGS='-D warnings' cargo clippy --workspace --all-targets -- -D warnings"
    )
    .run()?;
    // `--all-targets` が lib テストを含むので個別の `-p` 実行は足さない。`-p` 単体は依存の feature 解決が
    // 変わり、同じテストのために依存ツリーを再ビルドするだけになる。
    step("cargo test");
    cmd!(
        shell,
        "env RUSTFLAGS='-D warnings' cargo test --workspace --all-targets"
    )
    .run()?;
    // これは feature 構成が既定と異なる（stub backend）ため workspace 実行に包含されない。
    step("cargo test secrets internal stub");
    cmd!(
        shell,
        "env RUSTFLAGS='-D warnings' cargo test -p dotfiles-cli --features secrets-internal-test-stub --test secrets_cli"
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

/// GitHub Actions workflow の構文と式を actionlint で検証する。
fn github_actions(shell: &Shell) -> Result<()> {
    step("GitHub Actions workflows");
    cmd!(shell, "actionlint").run()?;
    Ok(())
}

/// lock file が存在する状態で、Nix flake の評価と Nix ファイルの整形を検証する。
fn nix(shell: &Shell) -> Result<()> {
    step("flake.lock exists");
    cmd!(shell, "test -s flake.lock").run()?;
    let files = nix_files(shell)?;
    if !files.is_empty() {
        step("nix fmt");
        cmd!(shell, "nix fmt -- --ci {files...}").run()?;
    }
    step("nix flake check");
    cmd!(shell, "nix flake check --no-update-lock-file --all-systems").run()?;
    // `nix flake check` は `darwinConfigurations` を出力名として列挙するだけで、その構成を評価しない。
    // `nix/darwin.nix` から Home Manager 子モジュールへ渡す module 引数が切れても素通りし、実際に評価する
    // nightly bump の `eval-versions` まで失敗が現れない。翌日の無人実行ではなく PR で落とすため、評価対象を
    // 検査側へ書き写さず、nightly が叩くのと同じ command をここでも起動する。JSON は評価が通ったことの
    // 副産物でしかないので捨てる。
    step("darwinConfigurations.ci-ref eval");
    let dotfiles = dotfiles_binary()?;
    let out_dir = shell.create_temp_dir()?;
    let out = out_dir.path().join("declared-versions.json");
    cmd!(
        shell,
        "{dotfiles} update-history eval-versions --reference {CI_REFERENCE} --out {out}"
    )
    .run()?;
    Ok(())
}

/// `rust()` の workspace ビルドが uplift した `dotfiles` binary を、自分と同じ target directory から引く。
///
/// `cargo run --package dotfiles-cli` で起動し直すと、package 選択が変わって feature 解決が
/// `cargo test --workspace --all-targets` と一致せず、依存ツリーが丸ごともう 1 世代コンパイルされる
/// （cold な target directory で 37 crate・約 40 秒）。この検証は同一ワークスペースの重複ビルドを削る
/// 一環なので、既にある成果物を起動する。
fn dotfiles_binary() -> Result<std::path::PathBuf> {
    let checks_binary = std::env::current_exe()?;
    let Some(directory) = checks_binary.parent() else {
        bail!("dotfiles-checks の実行ファイル位置から target directory を解決できませんでした");
    };
    Ok(directory.join("dotfiles"))
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
