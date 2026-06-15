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
    darwin_home_manager_propagates_include_self_package(&shell)?;
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
    nightly_record_rebuilds_in_job(shell)?;
    nightly_bump_artifact_preserves_old_lock_and_base_sha_wiring(shell)?;
    nightly_lock_rev_skips_nix_develop(shell)?;
    nightly_artifact_actions_use_supported_node_runtime(shell)?;
    Ok(())
}

/// `mkDarwin` から Home Manager の子モジュールへ `includeSelfPackage` が落ちずに届くことを固定する。
///
/// `darwinModule` 自体で `_module.args.includeSelfPackage` を持っていても、`nix/darwin.nix` が
/// `home-manager.extraSpecialArgs` へ同値を渡し忘れると、`home.nix -> modules/cli.nix` の評価だけが
/// `attribute 'includeSelfPackage' missing` で落ちる。nightly の `darwinConfigurations.ci-ref` eval は
/// まさにこの経路を踏むため、静的検査で配線抜けを止める。
fn darwin_home_manager_propagates_include_self_package(shell: &Shell) -> Result<()> {
    step("darwin home-manager includeSelfPackage propagation");
    let darwin = shell.read_file("nix/darwin.nix")?;
    ensure!(
        darwin.contains("includeSelfPackage ? true,"),
        "nix/darwin.nix は `includeSelfPackage` をモジュール引数で受け取り、mkDarwin 既定値を保持すること"
    );
    let extra_special_args = darwin
        .split("home-manager.extraSpecialArgs =")
        .nth(1)
        .and_then(|section| section.split("home-manager.users.").next())
        .unwrap_or_default();
    ensure!(
        extra_special_args.contains("includeSelfPackage"),
        "nix/darwin.nix は `home-manager.extraSpecialArgs` へ `includeSelfPackage` を渡し、\
         home.nix -> modules/cli.nix の評価で欠落させないこと"
    );
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

/// nightly-update.yml の record 要約経路が「default branch ref に限定された secret 注入」になっていることを固定する。
///
/// `OPEN_AI_API_KEY` を workflow_dispatch の任意 ref に戻すと、未審査 ref の Rust/Nix コードへ secret を
/// 渡せる。そこで record job の secret 注入は `schedule` または `workflow_dispatch && github.actor ==
/// github.repository_owner && github.ref == default_branch` に限定し、未審査 ref の dry-run では version-only
/// に倒す。open-pr job 側の既定ブランチ制限と合わせて、secret を使う build/record 経路全体を既定ブランチへ
/// 閉じ込める。
fn nightly_record_secret_gating_is_testable_and_bounded(shell: &Shell) -> Result<()> {
    step("nightly-update record secret gating");
    let workflow = shell.read_file(".github/workflows/nightly-update.yml")?;
    assert_nightly_record_secret_gating_is_testable_and_bounded(&workflow)
}

#[cfg(test)]
fn record_secret_gate_allows(
    event_name: &str,
    actor: &str,
    repository_owner: &str,
    git_ref: &str,
    default_branch: &str,
) -> bool {
    event_name == "schedule"
        || (event_name == "workflow_dispatch"
            && actor == repository_owner
            && git_ref == format!("refs/heads/{default_branch}"))
}

fn assert_nightly_record_secret_gating_is_testable_and_bounded(workflow: &str) -> Result<()> {
    ensure!(
        workflow.contains(
            "OPEN_AI_API_KEY: ${{ (github.event_name == 'schedule' || (github.event_name == 'workflow_dispatch' && github.actor == github.repository_owner && github.ref == format('refs/heads/{0}', github.event.repository.default_branch))) && secrets.OPEN_AI_API_KEY || '' }}"
        ),
        "record job の OPEN_AI_API_KEY は schedule または repo owner の default branch workflow_dispatch に限定し、\
         未審査 ref の dry-run へ secret を渡さないこと"
    );
    ensure!(
        workflow.contains(
            "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)) }}"
        ),
        "open-pr job の既定ブランチ限定は維持し、PR 起票/status 投稿経路の信頼境界を弱めてはならない"
    );
    Ok(())
}

/// nightly-update.yml の record job が同一 job で dotfiles binary を再ビルドし、job 間で持ち回した binary の
/// 動的ライブラリ参照切れに依存しないことを静的に固定する。
fn nightly_record_rebuilds_in_job(shell: &Shell) -> Result<()> {
    step("nightly-update record rebuilds binary in job");
    let workflow = shell.read_file(".github/workflows/nightly-update.yml")?;
    assert_nightly_record_rebuilds_in_job(&workflow)
}

fn assert_nightly_record_rebuilds_in_job(workflow: &str) -> Result<()> {
    let record_section = workflow
        .split("- name: record（nix/brew 版差分 + 概要）")
        .nth(1)
        .unwrap_or_default();
    let record_step = record_section.split("- name:").next().unwrap_or_default();
    ensure!(
        workflow.contains("- name: record 用 dotfiles バイナリをビルド")
            && workflow.contains("nix develop -c cargo build -p dotfiles-cli"),
        "record job は同一 job の devShell で dotfiles binary を再ビルドすること"
    );
    ensure!(
        record_step.contains("dotfiles_bin=\"$PWD/target/debug/dotfiles\""),
        "record job は同一 job でビルドした target/debug/dotfiles を使うこと"
    );
    ensure!(
        !workflow.contains("chmod +x target/debug/dotfiles"),
        "record job は artifact binary の実行ビット復元に依存してはならない"
    );
    ensure!(
        !workflow.contains("bump 前 eval 版マップと dotfiles binary を取得"),
        "record job の artifact download は binary を前提にしてはならない"
    );
    Ok(())
}

/// nightly-update.yml の bump artifact が `old-flake.lock` と `repo_base_sha` を保持し、record/open-pr へ
/// それぞれ `--lock-old/--lock-new` と `BUMP_BASE_SHA` で受け渡されることを静的に固定する。
fn nightly_bump_artifact_preserves_old_lock_and_base_sha_wiring(shell: &Shell) -> Result<()> {
    step("nightly-update bump artifact preserves old lock and base sha wiring");
    let workflow = shell.read_file(".github/workflows/nightly-update.yml")?;
    assert_nightly_bump_artifact_preserves_old_lock_and_base_sha_wiring(&workflow)
}

fn assert_nightly_bump_artifact_preserves_old_lock_and_base_sha_wiring(
    workflow: &str,
) -> Result<()> {
    let old_eval_section = workflow
        .split("- name: bump 前の宣言パッケージ版を eval と rev 抽出")
        .nth(1)
        .unwrap_or_default();
    let old_eval_step = old_eval_section.split("- name:").next().unwrap_or_default();
    ensure!(
        old_eval_step.contains("cp flake.lock old-flake.lock"),
        "bump job は flake update 前に `cp flake.lock old-flake.lock` で旧 lock を保存すること"
    );
    ensure!(
        old_eval_step
            .contains("echo \"repo_base_sha=$(git rev-parse HEAD)\" >> \"$GITHUB_OUTPUT\""),
        "bump job は artifact 作成時点の checkout HEAD を `repo_base_sha` output として公開すること"
    );

    let bump_artifact_section = workflow
        .split("- name: bump 済み lock と eval 版マップを artifact 化")
        .nth(1)
        .unwrap_or_default();
    let bump_artifact_step = bump_artifact_section
        .split("- name:")
        .next()
        .unwrap_or_default();
    ensure!(
        bump_artifact_step.contains("name: bump-state"),
        "bump job は record/open-pr 共有用に `bump-state` artifact を publish すること"
    );
    ensure!(
        bump_artifact_step.contains("old-flake.lock"),
        "bump-state artifact は `old-flake.lock` を含み、record job へ旧 lock を渡すこと"
    );
    ensure!(
        bump_artifact_step.contains("flake.lock"),
        "bump-state artifact は bump 後 `flake.lock` も含むこと"
    );

    let record_section = workflow
        .split("- name: record（nix/brew 版差分 + 概要）")
        .nth(1)
        .unwrap_or_default();
    let record_step = record_section.split("- name:").next().unwrap_or_default();
    ensure!(
        record_step.contains("--lock-old old-flake.lock"),
        "record job は bump artifact から展開した `old-flake.lock` を `--lock-old` で渡すこと"
    );
    ensure!(
        record_step.contains("--lock-new flake.lock"),
        "record job は bump 後 `flake.lock` を `--lock-new` で渡すこと"
    );

    ensure!(
        workflow.contains("repo_base_sha: ${{ steps.old.outputs.repo_base_sha }}"),
        "bump job outputs は `steps.old.outputs.repo_base_sha` を `repo_base_sha` として公開すること"
    );
    ensure!(
        workflow.contains("BUMP_BASE_SHA: ${{ needs.bump.outputs.repo_base_sha }}"),
        "open-pr job は `needs.bump.outputs.repo_base_sha` を `BUMP_BASE_SHA` へ配線すること"
    );
    ensure!(
        workflow.contains("if [ \"$base_sha\" != \"$BUMP_BASE_SHA\" ]; then"),
        "open-pr job は `BUMP_BASE_SHA` と現在の default branch HEAD を比較して fail-closed にすること"
    );
    Ok(())
}

/// nightly-update.yml の lock-rev 抽出が `nix develop` を不要に挟まず、純粋な lock file parse として直接実行される
/// ことを静的に固定する。
fn nightly_lock_rev_skips_nix_develop(shell: &Shell) -> Result<()> {
    step("nightly-update lock-rev skips nix develop");
    let workflow = shell.read_file(".github/workflows/nightly-update.yml")?;
    assert_nightly_lock_rev_skips_nix_develop(&workflow)
}

fn assert_nightly_lock_rev_skips_nix_develop(workflow: &str) -> Result<()> {
    ensure!(
        workflow
            .contains("\"$DOTFILES_BIN\" update-history lock-rev --lock flake.lock --node nixpkgs"),
        "lock-rev は built dotfiles binary を直接実行すること"
    );
    ensure!(
        workflow.contains("\"$DOTFILES_BIN\" update-history lock-rev --lock flake.lock --node homebrew-homebrew-cask"),
        "cask rev 抽出も built dotfiles binary を直接実行すること"
    );
    ensure!(
        !workflow.contains("nix develop -c \"$DOTFILES_BIN\" update-history lock-rev"),
        "lock-rev は `nix develop` を挟まず直接実行し、不要な shell 起動で bump を遅くしてはならない"
    );
    Ok(())
}

/// nightly-update.yml の artifact action が Node 20 廃止 warning の出る古い major に戻らないことを静的に固定する。
fn nightly_artifact_actions_use_supported_node_runtime(shell: &Shell) -> Result<()> {
    step("nightly-update artifact actions avoid node20 deprecation");
    let workflow = shell.read_file(".github/workflows/nightly-update.yml")?;
    assert_nightly_artifact_actions_use_supported_node_runtime(&workflow)
}

fn assert_nightly_artifact_actions_use_supported_node_runtime(workflow: &str) -> Result<()> {
    ensure!(
        workflow.contains("actions/upload-artifact@v7"),
        "nightly-update は Node 20 廃止 warning を避けるため upload-artifact@v7 を使うこと"
    );
    ensure!(
        workflow.contains("actions/download-artifact@v8"),
        "nightly-update は Node 20 廃止 warning を避けるため download-artifact@v8 を使うこと"
    );
    ensure!(
        !workflow.contains("actions/upload-artifact@v4"),
        "nightly-update は upload-artifact@v4 へ戻してはならない"
    );
    ensure!(
        !workflow.contains("actions/download-artifact@v4"),
        "nightly-update は download-artifact@v4 へ戻してはならない"
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
        assert_nightly_artifact_actions_use_supported_node_runtime,
        assert_nightly_bump_artifact_preserves_old_lock_and_base_sha_wiring,
        assert_nightly_lock_rev_skips_nix_develop, assert_nightly_record_rebuilds_in_job,
        assert_nightly_record_secret_gating_is_testable_and_bounded, record_secret_gate_allows,
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

    /// record job の OpenAI secret は repo owner の manual dispatch でも default branch ref に限定される。
    #[test]
    fn nightly_record_secret_gating_accepts_owner_default_branch_dispatch_and_keeps_open_pr_gate() {
        let workflow = r#"
          OPEN_AI_API_KEY: ${{ (github.event_name == 'schedule' || (github.event_name == 'workflow_dispatch' && github.actor == github.repository_owner && github.ref == format('refs/heads/{0}', github.event.repository.default_branch))) && secrets.OPEN_AI_API_KEY || '' }}
          if: >-
            ${{ github.event_name == 'schedule' ||
                (github.event_name == 'workflow_dispatch' &&
                 github.event.inputs.dry_run == 'false' &&
                 github.ref == format('refs/heads/{0}', github.event.repository.default_branch)) }}
        "#;

        assert!(assert_nightly_record_secret_gating_is_testable_and_bounded(workflow).is_ok());
    }

    #[test]
    fn record_secret_gate_rejects_owner_non_default_branch_dispatch() {
        assert!(!record_secret_gate_allows(
            "workflow_dispatch",
            "owner",
            "owner",
            "refs/heads/feature",
            "main"
        ));
    }

    #[test]
    fn record_secret_gate_accepts_owner_default_branch_dispatch() {
        assert!(record_secret_gate_allows(
            "workflow_dispatch",
            "owner",
            "owner",
            "refs/heads/main",
            "main"
        ));
    }

    /// record job の OpenAI secret を owner の任意 dispatch へ戻す退行は、未審査 ref へ secret が流れるため拒否する。
    #[test]
    fn nightly_record_secret_gating_rejects_non_default_branch_dispatch_regression() {
        let workflow = r#"
          OPEN_AI_API_KEY: ${{ (github.event_name == 'schedule' || (github.event_name == 'workflow_dispatch' && github.actor == github.repository_owner)) && secrets.OPEN_AI_API_KEY || '' }}
          if: >-
            ${{ github.event_name == 'schedule' ||
                (github.event_name == 'workflow_dispatch' &&
                 github.event.inputs.dry_run == 'false' &&
                 github.ref == format('refs/heads/{0}', github.event.repository.default_branch)) }}
        "#;

        let result = assert_nightly_record_secret_gating_is_testable_and_bounded(workflow);
        assert!(result.is_err());
    }

    #[test]
    fn nightly_record_rebuilds_binary_in_job() {
        let workflow = r#"
          - name: record 用 dotfiles バイナリをビルド
            run: nix develop -c cargo build -p dotfiles-cli
          - name: record（nix/brew 版差分 + 概要）
            run: |
              dotfiles_bin="$PWD/target/debug/dotfiles"
              nix develop -c "$dotfiles_bin" update-history record \
                --out "$out"
        "#;

        assert!(assert_nightly_record_rebuilds_in_job(workflow).is_ok());
    }

    #[test]
    fn nightly_bump_artifact_preserves_old_lock_and_base_sha_wiring() {
        let workflow = r#"
          outputs:
            repo_base_sha: ${{ steps.old.outputs.repo_base_sha }}
          - name: bump 前の宣言パッケージ版を eval と rev 抽出
            run: |
              cp flake.lock old-flake.lock
              echo "repo_base_sha=$(git rev-parse HEAD)" >> "$GITHUB_OUTPUT"
          - name: bump 済み lock と eval 版マップを artifact 化
            with:
              name: bump-state
              path: |
                flake.lock
                old-flake.lock
                nix-old.json
          - name: record（nix/brew 版差分 + 概要）
            run: |
              nix develop -c "$dotfiles_bin" update-history record \
                --lock-old old-flake.lock \
                --lock-new flake.lock \
                --out "$out"
          - name: bump ブランチを作成して commit
            env:
              BUMP_BASE_SHA: ${{ needs.bump.outputs.repo_base_sha }}
            run: |
              if [ "$base_sha" != "$BUMP_BASE_SHA" ]; then
                exit 1
              fi
        "#;

        assert!(
            assert_nightly_bump_artifact_preserves_old_lock_and_base_sha_wiring(workflow).is_ok()
        );
    }

    #[test]
    fn nightly_bump_artifact_rejects_missing_old_lock_and_base_sha_wiring() {
        let workflow = r#"
          outputs:
            repo_base_sha: ${{ steps.old.outputs.repo_base_sha }}
          - name: bump 前の宣言パッケージ版を eval と rev 抽出
            run: |
              echo "repo_base_sha=$(git rev-parse HEAD)" >> "$GITHUB_OUTPUT"
          - name: bump 済み lock と eval 版マップを artifact 化
            with:
              name: bump-state
              path: |
                flake.lock
                nix-old.json
          - name: record（nix/brew 版差分 + 概要）
            run: |
              nix develop -c "$dotfiles_bin" update-history record \
                --lock-new flake.lock \
                --out "$out"
          - name: bump ブランチを作成して commit
            env:
              BUMP_BASE_SHA: ${{ github.sha }}
            run: |
              if [ "$base_sha" != "$BUMP_BASE_SHA" ]; then
                exit 1
              fi
        "#;

        assert!(
            assert_nightly_bump_artifact_preserves_old_lock_and_base_sha_wiring(workflow).is_err()
        );
    }

    #[test]
    fn nightly_lock_rev_runs_without_nix_develop() {
        let workflow = r#"
          nixpkgs_old="$("$DOTFILES_BIN" update-history lock-rev --lock flake.lock --node nixpkgs)"
          cask_rev_old="$("$DOTFILES_BIN" update-history lock-rev --lock flake.lock --node homebrew-homebrew-cask)"
          nixpkgs_new="$("$DOTFILES_BIN" update-history lock-rev --lock flake.lock --node nixpkgs)"
          cask_rev_new="$("$DOTFILES_BIN" update-history lock-rev --lock flake.lock --node homebrew-homebrew-cask)"
        "#;

        assert!(assert_nightly_lock_rev_skips_nix_develop(workflow).is_ok());
    }

    #[test]
    fn nightly_artifact_actions_use_supported_node_runtime() {
        let workflow = r#"
          - uses: actions/upload-artifact@v7
          - uses: actions/download-artifact@v8
        "#;

        assert!(assert_nightly_artifact_actions_use_supported_node_runtime(workflow).is_ok());
    }
}
