//! VM を使わずに実行できる静的検証。
//!
//! 実体は標準のリンター群と、リンターが評価しない構成の評価だけである。`nix flake check` は
//! `darwinConfigurations` / `homeConfigurations` を出力名として列挙するだけで構成を評価しないため、
//! 評価は別ステップで行う。
//!
//! ソーステキストを解析する検査は置かない（`docs/docs-governance.md` を参照）。

use anyhow::{bail, ensure};
use xshell::{Shell, cmd};

use crate::Result;

/// nightly bump が版差分の算出対象にする参照構成。`nightly-update.yml` の `CI_REFERENCE` と同じ値を指す。
const CI_REFERENCE: &str = "darwinConfigurations.ci-ref";

/// 長い検証ログで失敗位置を追えるよう、各検証ブロックの開始を同じ形式で出力する。
fn step(label: &str) {
    println!("==> {label}");
}

/// dirty な実マシン状態に依存しない、リポジトリ内だけで完結する検証を実行する。
pub(crate) fn check() -> Result<()> {
    let shell = Shell::new()?;
    rust(&shell)?;
    shell_scripts(&shell)?;
    github_actions(&shell)?;
    nix_diagnostics(&shell)?;
    nix(&shell)?;
    home_configuration_evaluates_for_every_system(&shell)
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

/// lock file が存在する状態で、Nix ファイルの整形と、flake 出力および `darwinConfigurations` の評価を検証する。
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
    // `nix/darwin.nix` から Home Manager 子モジュールへ渡す module 引数が切れても素通りするため、nightly が
    // 叩くのと同じ command をここでも起動する。JSON は評価が通ったことの副産物でしかないので捨てる。
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

/// `lib.mkHome` が評価対象 system で成立することを、ホーム配置とパッケージ集合の両方で確認する。
///
/// ホーム配置は生成物へ焼き込まれる。実行環境の実ホームと食い違うと、起動した zsh が存在しないパスへ
/// 書きに行く。`activationPackage` の `drvPath` は全モジュールの評価結果を入力に持つため、これを引けば
/// `home.packages` も `home.file` も forcing され、片方の system だけで通る宣言が落ちる。ビルドは伴わない
/// ので runner の OS を問わない。
fn home_configuration_evaluates_for_every_system(shell: &Shell) -> Result<()> {
    step("lib.mkHome が両 system で成立する");
    for (system, expected) in [
        ("aarch64-darwin", "/Users/runner"),
        ("x86_64-linux", "/home/runner"),
    ] {
        let home = format!(
            "f: (f {{ user = \"runner\"; system = \"{system}\"; }}).config.home.homeDirectory"
        );
        let actual = cmd!(
            shell,
            "nix eval --raw --no-update-lock-file .#lib.mkHome --apply {home}"
        )
        .read()?;
        ensure!(
            actual == expected,
            "system {system} の home ディレクトリが {actual}。{expected} を期待する"
        );

        let activation = format!(
            "f: (f {{ user = \"runner\"; system = \"{system}\"; }}).activationPackage.drvPath"
        );
        cmd!(
            shell,
            "nix eval --raw --no-update-lock-file .#lib.mkHome --apply {activation}"
        )
        .read()?;
    }
    Ok(())
}

/// `rust()` の workspace ビルドが uplift した `dotfiles` binary を、自分と同じ target directory から引く。
///
/// `cargo run --package dotfiles-cli` で起動し直すと package 選択が変わり、feature 解決が
/// `cargo test --workspace --all-targets` と一致せず依存ツリーがもう 1 世代コンパイルされる。
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
