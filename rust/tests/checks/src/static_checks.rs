//! VM を使わずに実行できる静的検証。
//!
//! Rust、shell script、Nix flake などの外部検証コマンドを順に実行する。

use anyhow::{bail, ensure};
use xshell::{Shell, cmd};

use crate::{Result, command::step};

/// dirty な実マシン状態に依存しない、リポジトリ内だけで完結する検証を実行する。
pub(crate) fn check() -> Result<()> {
    let shell = Shell::new()?;
    rust(&shell)?;
    shell_scripts(&shell)?;
    github_actions(&shell)?;
    nix_diagnostics(&shell)?;
    nix_derive_repo(&shell)?;
    nix(&shell)
}

/// `eval-declared-versions.sh` が import する `derive-repo.nix` の owner/repo 導出を fixture で固定する。
///
/// owner/repo 導出（homepage(github)→src→changelog(github) の優先・`.git` 剥がし・非 github→空）は nightly の
/// nix 版差分でリリースノート取得元を決める純関数で、実フリート構成や network に依らず評価できる。実 darwin
/// 構成を eval せず、`nix eval --expr` に自作 fixture package を与えて 4 分岐を hermetic に固定する（script と
/// テストは同一 `derive-repo.nix` を共有するため規則がドリフトしない。実ビルド/フェッチは走らない）。
fn nix_derive_repo(shell: &Shell) -> Result<()> {
    step("nix derive-repo owner/repo branches");

    // pure eval ではフレーク外パス import が禁じられるため、script と同じくファイル内容を inline 注入する
    // （実装の単一正本は derive-repo.nix で、内容を読み込んで式へ埋め込む）。
    let derive = shell.read_file(".github/scripts/derive-repo.nix")?;

    // ① homepage が github → そこから owner/repo（src/changelog より優先）。
    assert_repo_of(
        shell,
        &derive,
        "{ meta.homepage = \"https://github.com/neovim/neovim\"; \
           src = { owner = \"src-owner\"; repo = \"src-repo\"; }; \
           meta.changelog = \"https://github.com/cl-owner/cl-repo\"; }",
        "neovim/neovim",
        "homepage(github) を最優先する",
    )?;

    // ② homepage が非 github → src の owner+repo から。
    assert_repo_of(
        shell,
        &derive,
        "{ meta.homepage = \"https://example.com/home\"; \
           src = { owner = \"BurntSushi\"; repo = \"ripgrep\"; }; }",
        "BurntSushi/ripgrep",
        "homepage が非 github なら src へ倒す",
    )?;

    // ③ homepage/src が無い → changelog の github URL から。末尾 `.git` は剥がす。
    assert_repo_of(
        shell,
        &derive,
        "{ meta.changelog = \"https://github.com/owner/proj.git\"; }",
        "owner/proj",
        "changelog(github) へフォールバックし .git を剥がす",
    )?;

    // ④ いずれも非 github → 空文字（version-only 行き）。
    assert_repo_of(
        shell,
        &derive,
        "{ meta.homepage = \"https://gitlab.com/o/r\"; \
           meta.changelog = \"https://example.com/changelog\"; }",
        "",
        "非 github は空文字へ縮退する",
    )?;

    Ok(())
}

/// fixture package 1 件へ `derive-repo.nix` の `repoOf` を適用し、期待 owner/repo（または空文字）を確かめる。
///
/// `derive`（derive-repo.nix の内容）を inline した `nix eval --expr` を評価する。network・実構成 eval を
/// 伴わない純評価で、`--raw` 出力を期待値と突き合わせる（pure eval のパス import 制約を避けるため inline）。
fn assert_repo_of(
    shell: &Shell,
    derive: &str,
    fixture: &str,
    expected: &str,
    context: &str,
) -> Result<()> {
    let expr = format!("({derive}).repoOf ({fixture})");
    let actual = match cmd!(shell, "nix eval --raw --expr {expr}").read() {
        Ok(value) => value,
        Err(error) => bail!("derive-repo.nix repoOf eval failed ({context}): {error}"),
    };
    ensure!(
        actual == expected,
        "derive-repo.nix repoOf mismatch ({context}): expected {expected:?}, got {actual:?}"
    );
    Ok(())
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
    step("cargo test secrets internal stub");
    cmd!(
        shell,
        "env RUSTFLAGS='-D warnings' cargo test -p dotfiles-cli --features secrets-internal-test-stub --test secrets_cli"
    )
    .run()?;
    step("cargo test secrets application");
    cmd!(
        shell,
        "env RUSTFLAGS='-D warnings' cargo test -p dotfiles-secrets --lib application"
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
