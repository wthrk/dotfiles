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
    auto_update_wrapper_uses_update_all_semantics(&shell)?;
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
    nightly_record_secret_gating_is_testable_and_bounded(shell)?;
    Ok(())
}

/// nightly-update.yml の「無更新の夜が clean no-op になる」不変条件を hermetic に固定する（finding 3368677388）。
///
/// 全 input が既に最新で nix/brew 差分も空の夜は run_record が更新履歴 TOML を書かず、record job の
/// history-record アップロード対象が 0 件になりうる。このとき record の upload-artifact が
/// `if-no-files-found: error` だと無更新夜が失敗扱いになり、clean no-op（PR 起票せず success）にならない。
/// アップロードを安全側（`warn`/`ignore`）にし、後段 open-pr の history-record download は無更新夜だけ
/// 失敗を許容（`continue-on-error` を record の `has_history != 'true'` でガード）することで、無更新夜が
/// 全体として no-op になりつつ、更新ありの夜の真の download 失敗は握り潰さないことを workflow テキスト上で
/// 固定する（network/nix 非依存・ファイル内容の静的検査のみ）。
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

    // record job は当月 history を書いたか（更新あり）を `has_history` output で後段へ渡すこと。これが無いと
    // open-pr 側で無更新夜と更新夜を区別できず、download 失敗を一律握り潰す回帰へ戻る。
    ensure!(
        workflow.contains("has_history: ${{ steps.record.outputs.has_history }}"),
        "record job は当月 history を書いたかを `has_history` output で公開すること（更新夜の download 失敗を \
         握り潰さないための分岐根拠）"
    );

    // open-pr の history-record download は、無更新夜（record が history を書かない）だけ artifact 不在を許容し、
    // 更新ありの夜（has_history=true）は download の一時失敗を握り潰さず fail-closed にすること。そのため
    // `continue-on-error` を `needs.record.outputs.has_history != 'true'` でガードする（無条件 `true` は禁止）。
    let download = workflow
        .split("name: 履歴 TOML を取得")
        .nth(1)
        .unwrap_or_default();
    let download_step = download.split("- name:").next().unwrap_or_default();
    ensure!(
        download_step
            .contains("continue-on-error: ${{ needs.record.outputs.has_history != 'true' }}"),
        "open-pr の history-record download は無更新夜だけ失敗を許容し更新夜は fail-closed にするため \
         `continue-on-error: ${{ needs.record.outputs.has_history != 'true' }}` でガードすること"
    );
    ensure!(
        !download_step.contains("continue-on-error: true"),
        "open-pr の history-record download は無条件 `continue-on-error: true` を使ってはならない \
         （更新夜の真の download 失敗を握り潰す）"
    );
    Ok(())
}

/// nightly-update.yml の record 要約経路が「PR 段階で検証可能だが無制限には開かない」ことを静的に固定する。
///
/// `OPEN_AI_API_KEY` を既定ブランチ ref 限定にすると、未マージ PR の workflow では要約付き履歴を検証できない。
/// 一方で常時注入へ戻すと、workflow_dispatch を叩ける任意 actor へ secret を広げる。そこで record job の
/// secret 注入は `schedule` または `workflow_dispatch && github.actor == github.repository_owner` に限定し、
/// repo owner の手動検証 run だけ要約付き履歴を許可する。PR 起票/status 投稿の信頼境界は open-pr job の
/// 既定ブランチ制限が継続して担う。
fn nightly_record_secret_gating_is_testable_and_bounded(shell: &Shell) -> Result<()> {
    step("nightly-update record secret gating");
    let workflow = shell.read_file(".github/workflows/nightly-update.yml")?;
    assert_nightly_record_secret_gating_is_testable_and_bounded(&workflow)
}

fn assert_nightly_record_secret_gating_is_testable_and_bounded(workflow: &str) -> Result<()> {
    ensure!(
        workflow.contains(
            "OPEN_AI_API_KEY: ${{ (github.event_name == 'schedule' || (github.event_name == 'workflow_dispatch' && github.actor == github.repository_owner)) && secrets.OPEN_AI_API_KEY || '' }}"
        ),
        "record job の OPEN_AI_API_KEY は schedule または repo owner の workflow_dispatch に限定し、\
         PR ブランチ dry-run でも要約付き履歴を検証できる形を維持すること"
    );
    ensure!(
        workflow.contains(
            "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)) }}"
        ),
        "open-pr job の既定ブランチ限定は維持し、PR 起票/status 投稿経路の信頼境界を弱めてはならない"
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

/// root auto-update wrapper が `dotfiles update` の既定 `all` 経路を保つことを静的に検証する。
fn auto_update_wrapper_uses_update_all_semantics(shell: &Shell) -> Result<()> {
    step("nix-darwin auto-update wrapper");
    let module = shell.read_file("nix/darwin.nix")?;
    assert_auto_update_wrapper_uses_update_all_semantics(&module)
}

/// wrapper 本体だけを見て、`update darwin` 固定への退行と `--user` 欠落を検出する。
fn assert_auto_update_wrapper_uses_update_all_semantics(module: &str) -> Result<()> {
    let wrapper = module
        .split("autoUpdateWrapper = pkgs.writeShellScript")
        .nth(1)
        .unwrap_or_default()
        .split("'';")
        .next()
        .unwrap_or_default();

    ensure!(
        wrapper.contains("${dotfilesBin} update \\"),
        "auto-update wrapper は target を省略して `dotfiles update` の既定 `all` を使うこと"
    );
    ensure!(
        !wrapper.contains("${dotfilesBin} update darwin"),
        "auto-update wrapper は `dotfiles update darwin` に固定してはならない"
    );
    ensure!(
        wrapper.contains("--user ${lib.escapeShellArg user}"),
        "root daemon からの更新では lock 更新と Home Manager を降格するため `--user` を渡すこと"
    );
    ensure!(
        wrapper.contains("--host ${lib.escapeShellArg host}"),
        "nix-darwin 出力名を固定するため `--host` を渡すこと"
    );
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

#[cfg(test)]
mod tests {
    use super::{
        assert_auto_update_wrapper_uses_update_all_semantics,
        assert_nightly_record_secret_gating_is_testable_and_bounded,
    };

    /// wrapper が target を省略し、root daemon 用の `--user` / `--host` を渡す形を受け入れる。
    #[test]
    fn auto_update_wrapper_accepts_default_update_target_with_user_and_host() {
        let module = r#"
          autoUpdateWrapper = pkgs.writeShellScript "${autoUpdateLabel}-wrapper" ''
            exec env HOME=${lib.escapeShellArg homeDir} ${dotfilesBin} update \
              --config-dir ${lib.escapeShellArg configDir} \
              --user ${lib.escapeShellArg user} \
              --host ${lib.escapeShellArg host}
          '';
        "#;

        assert!(assert_auto_update_wrapper_uses_update_all_semantics(module).is_ok());
    }

    /// `update darwin` へ戻すと root daemon の all semantics が崩れるため検出する。
    #[test]
    fn auto_update_wrapper_rejects_darwin_target_regression() {
        let module = r#"
          autoUpdateWrapper = pkgs.writeShellScript "${autoUpdateLabel}-wrapper" ''
            exec env HOME=${lib.escapeShellArg homeDir} ${dotfilesBin} update darwin \
              --config-dir ${lib.escapeShellArg configDir} \
              --user ${lib.escapeShellArg user} \
              --host ${lib.escapeShellArg host}
          '';
        "#;

        assert!(assert_auto_update_wrapper_uses_update_all_semantics(module).is_err());
    }

    /// record job の OpenAI secret は repo owner の manual dispatch でも使える一方、open-pr の default-branch
    /// gate は維持されることを受け入れる。
    #[test]
    fn nightly_record_secret_gating_accepts_owner_dispatch_and_keeps_open_pr_gate() {
        let workflow = r#"
          OPEN_AI_API_KEY: ${{ (github.event_name == 'schedule' || (github.event_name == 'workflow_dispatch' && github.actor == github.repository_owner)) && secrets.OPEN_AI_API_KEY || '' }}
          if: >-
            ${{ github.event_name == 'schedule' ||
                (github.event_name == 'workflow_dispatch' &&
                 github.event.inputs.dry_run == 'false' &&
                 github.ref == format('refs/heads/{0}', github.event.repository.default_branch)) }}
        "#;

        assert!(assert_nightly_record_secret_gating_is_testable_and_bounded(workflow).is_ok());
    }

    /// record job の OpenAI secret を default branch ref 限定へ戻す退行は、PR 段階で要約付き履歴を検証できないため
    /// 拒否する。
    #[test]
    fn nightly_record_secret_gating_rejects_default_branch_only_regression() {
        let workflow = r#"
          OPEN_AI_API_KEY: ${{ github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && secrets.OPEN_AI_API_KEY || '' }}
          if: >-
            ${{ github.event_name == 'schedule' ||
                (github.event_name == 'workflow_dispatch' &&
                 github.event.inputs.dry_run == 'false' &&
                 github.ref == format('refs/heads/{0}', github.event.repository.default_branch)) }}
        "#;

        let result = assert_nightly_record_secret_gating_is_testable_and_bounded(workflow);
        assert!(result.is_err());
    }
}
