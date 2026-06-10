//! VM を使わずに実行できる静的検証。
//!
//! Rust、shell script、Nix flake などの外部検証コマンドを順に実行する。

use anyhow::ensure;
use xshell::{Shell, cmd};

use crate::{Result, command::step};

/// dirty な実マシン状態に依存しない、リポジトリ内だけで完結する検証を実行する。
pub(crate) fn check() -> Result<()> {
    let shell = Shell::new()?;
    rust(&shell)?;
    shell_scripts(&shell)?;
    github_actions(&shell)?;
    nix_diagnostics(&shell)?;
    nix(&shell)
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
    nightly_no_update_is_clean_no_op(shell)?;
    Ok(())
}

/// nightly-update.yml の「無更新の夜が clean no-op になる」不変条件を hermetic に固定する（finding 3368677388）。
///
/// 全 input が既に最新で nix/brew 差分も空の夜は run_record が更新履歴 TOML を書かず、record job の
/// history-record アップロード対象が 0 件になりうる。このとき record の upload-artifact が
/// `if-no-files-found: error` だと無更新夜が失敗扱いになり、clean no-op（PR 起票せず success）にならない。
/// アップロードを安全側（`warn`/`ignore`）にし、後段 open-pr の history-record download を
/// `continue-on-error` で受けて artifact 不在を許容することで、無更新夜が全体として no-op になることを
/// workflow テキスト上で固定する（network/nix 非依存・ファイル内容の静的検査のみ）。
fn nightly_no_update_is_clean_no_op(shell: &Shell) -> Result<()> {
    step("nightly-update no-op (history upload not fail-on-empty)");
    let workflow = shell.read_file(".github/workflows/nightly-update.yml")?;

    // history-record の upload-artifact ステップ本体だけを切り出し、その `if-no-files-found` が `error` でない
    // ことを確認する（無更新夜の 0 件アップロードを失敗扱いにしない）。判定対象を当該ステップ（`- name: 履歴
    // TOML を artifact 化` から次の `- name:` の手前まで）にスコープし、後続に別 upload ステップが追加されても
    // その `if-no-files-found:` を拾わないようにする。安全側の `warn`/`ignore` のいずれかを要求する。
    let upload_section = workflow
        .split("- name: 履歴 TOML を artifact 化")
        .nth(1)
        .unwrap_or_default();
    let upload = upload_section.split("- name:").next().unwrap_or_default();
    ensure!(
        !upload.contains("if-no-files-found: error"),
        "record の history-record アップロードは無更新夜（0 件）を失敗扱いにしないため \
         `if-no-files-found: error` を使ってはならない（warn/ignore で clean no-op にする）"
    );
    ensure!(
        upload.contains("if-no-files-found: warn") || upload.contains("if-no-files-found: ignore"),
        "record の history-record アップロードは `if-no-files-found: warn`/`ignore` で 0 件を許容すること"
    );

    // open-pr の history-record download は、無更新夜で artifact が作られない場合に job を赤くしないため
    // `continue-on-error: true` を伴うこと（download 失敗を許容して no-op フローへ倒す）。
    let download = workflow
        .split("name: 履歴 TOML を取得")
        .nth(1)
        .unwrap_or_default();
    let download_step = download.split("- name:").next().unwrap_or_default();
    ensure!(
        download_step.contains("continue-on-error: true"),
        "open-pr の history-record download は無更新夜の artifact 不在を許容するため \
         `continue-on-error: true` を伴うこと（PR 起票せず no-op へ倒す）"
    );
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
