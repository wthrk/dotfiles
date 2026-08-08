//! VM を使わずに実行できる静的検証。
//!
//! Rust、shell script、Nix flake などの外部検証コマンドを順に実行する。

use std::collections::BTreeMap;

use anyhow::{anyhow, ensure};
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
    nightly_lock_input_sources_match_expected_table(&shell)?;
    homebrew_cleanup_matches_locked_brew_capability(&shell)?;
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
    nightly_bump_updates_every_input(shell)?;
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
/// それぞれ `--lock-old/--lock-new` + `--cursor-old` と `BUMP_BASE_SHA` で受け渡されることを静的に固定する。
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
        record_step.contains("--cursor-old \"$REPO_BASE_SHA\""),
        "record job は legacy show --rev 互換のため `repo_base_sha` を `--cursor-old` で渡すこと"
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

/// nightly-update.yml の bump step が input を列挙せず、`nix flake update`（引数なし）で全 input を bump する
/// ことを静的に固定する。
///
/// input を列挙して一部を除外すると、除外分だけが据え置かれたまま他が前進し、上流が検証していない組み合わせへ
/// 収束する。実際に `brew-src` が `5.1.1` に据え置かれたまま `homebrew-cask` だけ前進し、現行 cask の
/// `depends_on :macos` を旧 brew が解釈できず `dotfiles update` の `brew bundle` 段が停止した。除外に対応する
/// 有人 bump 経路も無いため、除外は「更新されない」と同義になる。列挙形式への退行をここで止める。
fn nightly_bump_updates_every_input(shell: &Shell) -> Result<()> {
    step("nightly-update bumps every flake input");
    let workflow = shell.read_file(".github/workflows/nightly-update.yml")?;
    assert_nightly_bump_updates_every_input(&workflow)
}

/// bump step を切り出すためのアンカー（workflow の step 名）。
///
/// この名前を変えると検査対象セクションを失うため、欠落を「空セクション → 実行行不一致」ではなく専用の
/// `Err` として区別できるようにする。区別できないと、step 名変更（アンカー破損）と bump 形式の実質的な退行
/// （input 列挙形式への差し戻し）が同じ失敗として現れ、原因追跡を誤らせる。
const BUMP_STEP_ANCHOR: &str = "- name: 全 input を bump";

/// bump step の実行行が引数なしの `nix flake update` 1 行だけかを判定する純関数。
///
/// [`BUMP_STEP_ANCHOR`] で step を切り出し、そこから次の step までを検査対象にする。アンカーが見つからない
/// 場合は「bump 形式の退行」とは別の専用 `Err` を返し、step 名変更と実質的な退行を取り違えさせない。
/// caller responsibility: `workflow` は `.github/workflows/nightly-update.yml` の全文であること
/// （step 単位に切り出した断片を渡すと、次 step 境界が無く検査範囲が広がる）。
fn assert_nightly_bump_updates_every_input(workflow: &str) -> Result<()> {
    let bump_section = workflow.split(BUMP_STEP_ANCHOR).nth(1).ok_or_else(|| {
        anyhow!(
            "nightly-update.yml に bump step `{BUMP_STEP_ANCHOR}` が見つからない。step 名を変更する場合は \
             本検査のアンカーも同時に更新すること（アンカー破損と bump 形式の退行を取り違えないため \
             fail-closed にする）"
        )
    })?;
    let bump_step = bump_section.split("- name:").next().unwrap_or_default();
    // コメント行を除いた実行行だけを見る。`nix flake update` を含む実行行は引数なしの 1 行に限る。
    let update_lines: Vec<&str> = bump_step
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#') && line.contains("nix flake update"))
        .collect();
    ensure!(
        update_lines == ["nix flake update"],
        "bump job は `nix flake update`（引数なし）1 行で flake.lock の全 input を bump すること。\
         input を列挙して一部を bump 対象から外すと、据え置き input と前進 input の未検証な組み合わせに \
         収束する（実行行: {update_lines:?}）"
    );
    Ok(())
}

/// 取得先期待値表（`rust/xtask/src/ci/bump_lock.rs` の `EXPECTED_LOCK_INPUT_SOURCES`）が実 `flake.lock` の
/// input を過不足なく網羅していることを静的に固定する。
///
/// nightly が `nix flake update`（引数なし）で全 input を bump するため、この表は「bump してよい input を
/// 選ぶ表」ではなく「実在 input の取得先期待値の写し」である。`flake.nix` へ input を 1 本足して表を更新し
/// 忘れると、その input が bump された翌晩に `verify-bump-lock` が
/// `has no expected source identity entry` で fail し、nightly PR が毎晩失敗して auto-merge が恒久停止する。
/// エラー文言は同期漏れではなくセキュリティ違反に読めるため原因追跡も難しい。手書き定数と実 lock の drift を
/// ここで止める。
///
/// この検査は表の**網羅性**だけを機械化する。取得先の同一性（owner/repo 厳密一致・source 座標）を実際に
/// 強制するのは `verify-bump-lock` 側であり、本検査はその表が実在 input と一致することだけを保証する。
fn nightly_lock_input_sources_match_expected_table(shell: &Shell) -> Result<()> {
    step("expected lock input source table covers every flake.lock input");
    let lock = shell.read_file("flake.lock")?;
    let guard = shell.read_file("rust/xtask/src/ci/bump_lock.rs")?;
    assert_lock_input_sources_match_expected_table(&lock, &guard)
}

/// 実 `flake.lock` の input 集合と期待取得先表を突合し、いずれかにしか無い input と owner/repo 不一致を検出する。
fn assert_lock_input_sources_match_expected_table(lock: &str, guard: &str) -> Result<()> {
    let expected = parse_expected_lock_input_sources(guard)?;
    let locked = lock_input_sources(lock)?;

    let missing: Vec<&str> = locked
        .keys()
        .filter(|name| !expected.contains_key(*name))
        .map(String::as_str)
        .collect();
    ensure!(
        missing.is_empty(),
        "flake.lock の input {missing:?} が rust/xtask/src/ci/bump_lock.rs の \
         `EXPECTED_LOCK_INPUT_SOURCES` に無い。nightly は全 input を bump するため、`flake.nix` へ input を \
         足したら同じ input 名と owner/repo を期待取得先表へも追加すること（未追加だと翌晩の \
         verify-bump-lock が `has no expected source identity entry` で fail し auto-merge が止まる）"
    );

    let stale: Vec<&str> = expected
        .keys()
        .filter(|name| !locked.contains_key(*name))
        .map(String::as_str)
        .collect();
    ensure!(
        stale.is_empty(),
        "`EXPECTED_LOCK_INPUT_SOURCES` の {stale:?} が現行 flake.lock に存在しない。input を削除・rename \
         したら期待取得先表からも同時に削除し、表を実在 input の写しに保つこと"
    );

    for (name, (owner, repo)) in &locked {
        let Some((expected_owner, expected_repo)) = expected.get(name) else {
            continue;
        };
        ensure!(
            expected_owner == owner && expected_repo == repo,
            "input `{name}` の owner/repo が flake.lock（{owner}/{repo}）と \
             `EXPECTED_LOCK_INPUT_SOURCES`（{expected_owner}/{expected_repo}）で一致しない。この表は取得先の \
             期待値であり、実 lock とずれたままだと verify-bump-lock が正当な bump を owner/repo 不一致として \
             fail させる"
        );
    }
    Ok(())
}

/// `EXPECTED_LOCK_INPUT_SOURCES` の配列リテラルから `(input 名, owner, repo)` を読み取る。
///
/// 定数名・配列終端が見つからない、または 1 件も読めない場合は `Err` にする（検査が空振りして網羅性の
/// invariant を黙って失うことを防ぐ）。
fn parse_expected_lock_input_sources(guard: &str) -> Result<BTreeMap<String, (String, String)>> {
    let table = guard
        .split("const EXPECTED_LOCK_INPUT_SOURCES")
        .nth(1)
        .and_then(|rest| rest.split_once("];"))
        .map(|(body, _)| body)
        .ok_or_else(|| {
            anyhow!(
                "rust/xtask/src/ci/bump_lock.rs に `EXPECTED_LOCK_INPUT_SOURCES` の配列定義が見つからない。\
                 定数名を変えるなら本検査も同時に更新すること（網羅性検査を空振りさせないため fail-closed）"
            )
        })?;
    let mut parsed = BTreeMap::new();
    for line in table.lines() {
        let Some(entry) = line
            .trim()
            .strip_prefix('(')
            .and_then(|rest| rest.split_once(')'))
            .map(|(entry, _)| entry)
        else {
            continue;
        };
        let fields: Vec<&str> = entry
            .split(',')
            .map(|field| field.trim().trim_matches('"'))
            .filter(|field| !field.is_empty())
            .collect();
        let [name, owner, repo] = fields[..] else {
            return Err(anyhow!(
                "`EXPECTED_LOCK_INPUT_SOURCES` の要素 `{entry}` が (input 名, owner, repo) の 3 要素ではない"
            ));
        };
        parsed.insert(name.to_owned(), (owner.to_owned(), repo.to_owned()));
    }
    ensure!(
        !parsed.is_empty(),
        "`EXPECTED_LOCK_INPUT_SOURCES` から 1 件も期待取得先を読めなかった。表の記法を変えるなら本検査も \
         更新すること"
    );
    Ok(parsed)
}

/// `flake.lock` の root 以外の全 node について、`locked` の owner/repo を取り出す。
fn lock_input_sources(lock: &str) -> Result<BTreeMap<String, (String, String)>> {
    let lock: serde_json::Value = serde_json::from_str(lock)?;
    let root = lock
        .get("root")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("flake.lock に root node 名が無い"))?;
    let nodes = lock
        .get("nodes")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| anyhow!("flake.lock に nodes object が無い"))?;
    let mut sources = BTreeMap::new();
    for (name, node) in nodes {
        if name == root {
            continue;
        }
        let locked = node
            .get("locked")
            .ok_or_else(|| anyhow!("flake.lock の node `{name}` に locked が無い"))?;
        let field = |key: &str| {
            locked
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        let (Some(owner), Some(repo)) = (field("owner"), field("repo")) else {
            return Err(anyhow!(
                "flake.lock の node `{name}` に locked.owner / locked.repo が無い。\
                 owner/repo を持たない input 形式を導入する場合は期待取得先表と verify-bump-lock の \
                 同一性検査も設計し直すこと"
            ));
        };
        sources.insert(name.clone(), (owner, repo));
    }
    Ok(sources)
}

/// `--force-cleanup` を渡す cleanup 方針が要求する brew 側 capability を持つと確認済みの最小 tag。
///
/// 下限を割る tag へ戻す必要が生じたら、`nix/modules/homebrew.nix` の cleanup 方針と同じ差分でこの値も
/// 更新すること（片方だけ動かすと switch 経路が実機でのみ壊れる）。
const BREW_REF_WITH_FORCE_CLEANUP: [u64; 3] = [6, 0, 13];

/// nix-darwin が `brew bundle` へ `--force-cleanup` を生成する `onActivation.cleanup` の値の集合。
///
/// nix-darwin の `modules/homebrew.nix` は `optional (cleanup == "uninstall") "--force-cleanup"` と
/// `optional (cleanup == "zap") "--zap --force-cleanup"` の 2 分岐でこのフラグを足す。したがって brew 版の
/// 下限は `uninstall` だけでなく `zap` にも掛かり、判定条件は個別の値ではなくこの集合で表す。
const CLEANUP_MODES_REQUIRING_FORCE_CLEANUP: [&str; 2] = ["uninstall", "zap"];

/// `--force-cleanup` を生成しない cleanup 方針として識別済みの値。
///
/// 現行 enum（`none` / `check` / `uninstall` / `zap`）のうち、brew 版下限と無関係だと実際に確認済みなのは
/// 旧 brew 向け迂回（`cleanup = "none"` + `extraFlags = [ "--cleanup" ]`、PR #87）の形だけである。それ以外の
/// 値は「brew のどの capability に依存するか未確認」として fail-closed にする。
const CLEANUP_MODE_WITHOUT_FORCE_CLEANUP: &str = "none";

/// `homebrew.nix` の cleanup 方針と lock 済み brew の版が両立していることを静的に固定する。
///
/// nix-darwin は cleanup が `uninstall` / `zap` のとき `brew bundle` へ `--force-cleanup` を渡す。この
/// フラグは brew 6.0.13 には存在するが、以前の 5.1.1 には無く、当時は `cleanup = "none"` +
/// `extraFlags = [ "--cleanup" ]` で迂回していた（PR #87）。全 input bump により `brew-src` の `ref` は無人で
/// 動くようになった一方、`verify-bump-lock` は推移 input の `ref` 差分を**方向を問わず**通す。つまり lock 側の
/// brew が下限を割る方向へ動いても guard は素通りし、switch 経路だけが実機で壊れる。
///
/// この検査は「switch 経路が brew の版に依存している」という設計判断を、lock 上の下限として機械的に固定する。
/// open-pr job は同一 run で `cargo xtask check static` を実行するため、下限を割る bump は `static checks`
/// status 未投稿となり無人 auto-merge されない（fail-closed）。下限を割る必要が生じた場合は
/// `homebrew.nix` の cleanup 方針と本定数を同じ差分で更新する。
fn homebrew_cleanup_matches_locked_brew_capability(shell: &Shell) -> Result<()> {
    step("homebrew cleanup mode matches locked brew capability");
    let module = shell.read_file("nix/modules/homebrew.nix")?;
    let lock = shell.read_file("flake.lock")?;
    assert_homebrew_cleanup_matches_locked_brew_capability(&module, &lock)
}

/// 宣言された cleanup 方針を識別し、`--force-cleanup` を生成する方針に限り brew tag の下限を要求する。
///
/// 「識別できない」状態（宣言が 1 件に確定しない、既知集合に無い値）は本検査が守る唯一の補償制御を無言で
/// 失う状態なので、`Ok` で素通りさせず `Err` にする。
fn assert_homebrew_cleanup_matches_locked_brew_capability(module: &str, lock: &str) -> Result<()> {
    let declarations = strip_nix_line_comments(module);
    let mode = declared_homebrew_cleanup_mode(&declarations)?;

    if !CLEANUP_MODES_REQUIRING_FORCE_CLEANUP.contains(&mode.as_str()) {
        ensure!(
            mode == CLEANUP_MODE_WITHOUT_FORCE_CLEANUP && declarations.contains(r#""--cleanup""#),
            "nix/modules/homebrew.nix の cleanup 方針 `{mode}` を本検査の既知集合\
             （{CLEANUP_MODES_REQUIRING_FORCE_CLEANUP:?} / `{CLEANUP_MODE_WITHOUT_FORCE_CLEANUP}` + \
             `--cleanup`）のどれとしても識別できない。この方針が brew のどの capability に依存するかを判断し、\
             依存するなら下限判定側へ、依存しないなら既知集合へ同じ差分で追加すること（未確認の方針を \
             無検査で通さないため fail-closed）"
        );
        return Ok(());
    }

    let reference = locked_brew_reference(lock)?;
    let version = parse_dotted_version(&reference).ok_or_else(|| {
        anyhow!(
            "flake.lock の brew-src `original.ref`（{reference}）を x.y.z として解釈できない。\
             `cleanup = \"{mode}\"` は brew 側の `--force-cleanup` に依存するため、tag 形式が変わったら \
             homebrew.nix の cleanup 方針と本検査の下限判定を同じ差分で更新すること"
        )
    })?;
    ensure!(
        version >= BREW_REF_WITH_FORCE_CLEANUP,
        "nix/modules/homebrew.nix は `cleanup = \"{mode}\"` を宣言しているが、flake.lock の brew-src は \
         {reference}（下限 {BREW_REF_WITH_FORCE_CLEANUP:?} 未満）に固定されている。この brew は \
         nix-darwin が渡す `--force-cleanup` を持たず `dotfiles update` の brew bundle 段が停止する。\
         brew を下限以上へ戻すか、homebrew.nix の cleanup 方針を同じ差分で変更すること"
    );
    Ok(())
}

/// Nix ソースから行コメント（`#` 以降）を落とし、説明文に書かれた宣言例を検査対象から除く。
///
/// `homebrew.nix` は cleanup 方針の履歴（旧 brew 向けの `cleanup = "none"` 迂回）をコメントで説明している
/// ため、コメントを残したまま宣言を探すと実宣言と説明を区別できない。`#` を含む文字列リテラルが同じ行に
/// 現れた場合は宣言が 1 件に確定しなくなり、`declared_homebrew_cleanup_mode` 側で fail-closed になる。
fn strip_nix_line_comments(module: &str) -> String {
    module
        .lines()
        .map(|line| line.split_once('#').map_or(line, |(code, _)| code))
        .collect::<Vec<&str>>()
        .join("\n")
}

/// `homebrew.nix` が実際に宣言している `onActivation.cleanup` の値を 1 件だけ取り出す。
///
/// 0 件（整形差・別ファイルへの分割・let 束縛経由などでアンカーが外れた）でも複数件でも `Err` にする。
/// ここを `Ok` で通すと、`verify-bump-lock` が推移 input の `ref` 差分を方向を問わず通すことへの唯一の
/// 補償制御が無言で dormant になる。
fn declared_homebrew_cleanup_mode(declarations: &str) -> Result<String> {
    const MARKER: &str = r#"cleanup = ""#;
    let modes: Vec<&str> = declarations
        .match_indices(MARKER)
        .filter_map(|(index, _)| declarations[index + MARKER.len()..].split('"').next())
        .collect();
    let [mode] = modes[..] else {
        return Err(anyhow!(
            "nix/modules/homebrew.nix から `onActivation.cleanup` の宣言を 1 件に確定できない（検出 {} 件）。\
             宣言形（整形・別ファイルへの分割・let 束縛経由など）を変える場合は本検査の判定条件も同じ差分で \
             更新すること（アンカーが外れたまま guard を dormant にしないため fail-closed）",
            modes.len()
        ));
    };
    Ok(mode.to_owned())
}

/// `flake.lock` の `brew-src` node が宣言する `original.ref`（親 flake 由来の brew tag）を取り出す。
fn locked_brew_reference(lock: &str) -> Result<String> {
    let lock: serde_json::Value = serde_json::from_str(lock)?;
    lock.get("nodes")
        .and_then(|nodes| nodes.get("brew-src"))
        .and_then(|node| node.get("original"))
        .and_then(|original| original.get("ref"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            anyhow!(
                "flake.lock に brew-src の `original.ref` が無い。nix-homebrew の input 構成が変わった場合は \
                 homebrew.nix の cleanup 方針が依存する brew 版の確認手段を設計し直すこと"
            )
        })
}

/// `x.y.z` 形式の版文字列を比較可能な数値 3 組へ変換する。解釈できなければ `None`。
fn parse_dotted_version(reference: &str) -> Option<[u64; 3]> {
    let parts: Vec<u64> = reference
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect::<Option<Vec<u64>>>()?;
    let [major, minor, patch] = parts[..] else {
        return None;
    };
    Some([major, minor, patch])
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
        assert_homebrew_cleanup_matches_locked_brew_capability,
        assert_lock_input_sources_match_expected_table,
        assert_nightly_bump_artifact_preserves_old_lock_and_base_sha_wiring,
        assert_nightly_bump_updates_every_input, assert_nightly_record_rebuilds_in_job,
        assert_nightly_record_secret_gating_is_testable_and_bounded, parse_dotted_version,
        record_secret_gate_allows,
    };

    /// 期待取得先表検査用の最小 lock（root input 1 本 + 推移 input 1 本）。
    fn lock_fixture() -> &'static str {
        r#"{
  "nodes": {
    "brew-src": {
      "flake": false,
      "locked": { "owner": "Homebrew", "repo": "brew", "rev": "aaaa", "type": "github" },
      "original": { "owner": "Homebrew", "ref": "6.0.13", "repo": "brew", "type": "github" }
    },
    "nix-homebrew": {
      "inputs": { "brew-src": "brew-src" },
      "locked": { "owner": "zhaofengli-wip", "repo": "nix-homebrew", "rev": "bbbb", "type": "github" },
      "original": { "owner": "zhaofengli-wip", "repo": "nix-homebrew", "type": "github" }
    },
    "root": { "inputs": { "nix-homebrew": "nix-homebrew" } }
  },
  "root": "root",
  "version": 7
}"#
    }

    /// 上の lock を過不足なく網羅する期待取得先表の Rust ソース断片。
    fn guard_fixture() -> &'static str {
        r#"
const EXPECTED_LOCK_INPUT_SOURCES: [(&str, &str, &str); 2] = [
    ("nix-homebrew", "zhaofengli-wip", "nix-homebrew"),
    ("brew-src", "Homebrew", "brew"),
];
"#
    }

    /// 引数なし `nix flake update` の bump step を受け入れる（全 input が bump 対象）。
    #[test]
    fn nightly_bump_accepts_argumentless_flake_update() {
        let workflow = r#"
      - name: 全 input を bump
        run: |
          set -euo pipefail
          # 引数を渡さず flake.lock の全 input を bump する。
          nix flake update

      - name: bump 後の input rev を抽出
"#;

        assert!(assert_nightly_bump_updates_every_input(workflow).is_ok());
    }

    /// input 列挙形式へ戻すと framework input が据え置かれるため検出する。
    #[test]
    fn nightly_bump_rejects_enumerated_input_list_regression() {
        let workflow = r#"
      - name: 全 input を bump
        run: |
          set -euo pipefail
          nix flake update \
            nixpkgs \
            homebrew-homebrew-cask

      - name: bump 後の input rev を抽出
"#;

        assert!(assert_nightly_bump_updates_every_input(workflow).is_err());
    }

    /// step 名アンカーが変わった場合は、bump 形式の退行とは区別できる専用エラーで fail-closed になる。
    #[test]
    fn nightly_bump_rejects_broken_step_anchor_distinguishably() {
        let workflow = r#"
      - name: flake input を bump
        run: |
          set -euo pipefail
          nix flake update

      - name: bump 後の input rev を抽出
"#;

        let err = assert_nightly_bump_updates_every_input(workflow).unwrap_err();
        assert!(
            err.to_string().contains("bump step") && err.to_string().contains("見つからない"),
            "{err}"
        );
    }

    /// 期待取得先表と lock の input 集合が一致していれば受理する。
    #[test]
    fn expected_source_table_accepts_exact_coverage() {
        assert!(
            assert_lock_input_sources_match_expected_table(lock_fixture(), guard_fixture()).is_ok()
        );
    }

    /// `flake.nix` に input を足して期待取得先表を更新し忘れた状態（lock にだけ input がある）を検出する。
    #[test]
    fn expected_source_table_rejects_input_missing_from_table() {
        let guard = r#"
const EXPECTED_LOCK_INPUT_SOURCES: [(&str, &str, &str); 1] = [
    ("nix-homebrew", "zhaofengli-wip", "nix-homebrew"),
];
"#;

        let err =
            assert_lock_input_sources_match_expected_table(lock_fixture(), guard).unwrap_err();
        assert!(err.to_string().contains("brew-src"), "{err}");
    }

    /// input を削除・rename したのに期待取得先表へ残っている状態も検出する。
    #[test]
    fn expected_source_table_rejects_stale_entry() {
        let guard = r#"
const EXPECTED_LOCK_INPUT_SOURCES: [(&str, &str, &str); 3] = [
    ("nix-homebrew", "zhaofengli-wip", "nix-homebrew"),
    ("brew-src", "Homebrew", "brew"),
    ("removed-tap", "someone", "homebrew-removed"),
];
"#;

        let err =
            assert_lock_input_sources_match_expected_table(lock_fixture(), guard).unwrap_err();
        assert!(err.to_string().contains("removed-tap"), "{err}");
    }

    /// 期待取得先表の owner/repo が実 lock とずれていれば検出する。
    #[test]
    fn expected_source_table_rejects_owner_mismatch() {
        let guard = r#"
const EXPECTED_LOCK_INPUT_SOURCES: [(&str, &str, &str); 2] = [
    ("nix-homebrew", "zhaofengli-wip", "nix-homebrew"),
    ("brew-src", "evil", "brew"),
];
"#;

        let err =
            assert_lock_input_sources_match_expected_table(lock_fixture(), guard).unwrap_err();
        assert!(err.to_string().contains("owner/repo"), "{err}");
    }

    /// 期待取得先表の定数名が変わって検査が空振りする状態は、黙って pass させず fail-closed にする。
    #[test]
    fn expected_source_table_rejects_missing_table_definition() {
        let err =
            assert_lock_input_sources_match_expected_table(lock_fixture(), "// no table here")
                .unwrap_err();
        assert!(err.to_string().contains("見つからない"), "{err}");
    }

    /// 表の要素が `(input 名, owner, repo)` の 3 要素でない形へ変わったら fail-closed にする。
    /// 読み飛ばし（`continue`）にすると、その entry だけ網羅性検査から静かに外れる。
    #[test]
    fn expected_source_table_rejects_entry_without_three_fields() {
        let guard = r#"
const EXPECTED_LOCK_INPUT_SOURCES: [(&str, &str); 1] = [
    ("nix-homebrew", "zhaofengli-wip"),
];
"#;

        let err =
            assert_lock_input_sources_match_expected_table(lock_fixture(), guard).unwrap_err();
        assert!(err.to_string().contains("3 要素ではない"), "{err}");
    }

    /// `locked` を持たない node 形式が現れたら fail-closed にする（読み飛ばすと取得先が未検査になる）。
    #[test]
    fn expected_source_table_rejects_node_without_locked_section() {
        let lock = lock_fixture().replace(
            r#""locked": { "owner": "Homebrew", "repo": "brew", "rev": "aaaa", "type": "github" },"#,
            "",
        );

        let err =
            assert_lock_input_sources_match_expected_table(&lock, guard_fixture()).unwrap_err();
        assert!(err.to_string().contains("locked が無い"), "{err}");
    }

    /// `locked.owner` を持たない取得先形式（非 github 等）も fail-closed にする。
    #[test]
    fn expected_source_table_rejects_node_without_locked_owner() {
        let lock = lock_fixture().replace(
            r#""owner": "Homebrew", "repo": "brew", "rev": "aaaa""#,
            r#""repo": "brew", "rev": "aaaa""#,
        );

        let err =
            assert_lock_input_sources_match_expected_table(&lock, guard_fixture()).unwrap_err();
        assert!(err.to_string().contains("locked.owner"), "{err}");
    }

    /// `locked.repo` 欠落も同様に fail-closed にする。
    #[test]
    fn expected_source_table_rejects_node_without_locked_repo() {
        let lock = lock_fixture().replace(r#""repo": "brew", "rev": "aaaa""#, r#""rev": "aaaa""#);

        let err =
            assert_lock_input_sources_match_expected_table(&lock, guard_fixture()).unwrap_err();
        assert!(err.to_string().contains("locked.repo"), "{err}");
    }

    /// root node 名を読めない lock は、root 除外ができず突合結果が意味を失うため fail-closed にする。
    #[test]
    fn expected_source_table_rejects_lock_without_root_node_name() {
        let lock = lock_fixture().replace("\n  \"root\": \"root\",", "");

        let err =
            assert_lock_input_sources_match_expected_table(&lock, guard_fixture()).unwrap_err();
        assert!(err.to_string().contains("root node 名"), "{err}");
    }

    /// `nodes` object を読めない lock も、空表として通さず fail-closed にする。
    #[test]
    fn expected_source_table_rejects_lock_without_nodes_object() {
        let lock = r#"{ "root": "root", "version": 7 }"#;

        let err =
            assert_lock_input_sources_match_expected_table(lock, guard_fixture()).unwrap_err();
        assert!(err.to_string().contains("nodes object"), "{err}");
    }

    /// `cleanup = "uninstall"` と下限以上の brew ref の組み合わせは受理する。
    #[test]
    fn homebrew_cleanup_accepts_force_cleanup_capable_brew() {
        let module = r#"      cleanup = "uninstall";"#;
        assert!(
            assert_homebrew_cleanup_matches_locked_brew_capability(module, lock_fixture()).is_ok()
        );
    }

    /// brew ref が下限を割る方向へ動くと、`--force-cleanup` を持たない brew と `cleanup = "uninstall"` の
    /// 組み合わせになるため fail させる（推移 input の `ref` 緩和が方向を問わないことへの補償）。
    #[test]
    fn homebrew_cleanup_rejects_brew_below_force_cleanup_floor() {
        let module = r#"      cleanup = "uninstall";"#;
        let lock = lock_fixture().replace(r#""ref": "6.0.13""#, r#""ref": "5.1.1""#);
        let err =
            assert_homebrew_cleanup_matches_locked_brew_capability(module, &lock).unwrap_err();
        assert!(err.to_string().contains("force-cleanup"), "{err}");
    }

    /// `cleanup = "zap"` も nix-darwin が `--zap --force-cleanup` を生成するため、同じ下限が掛かる。
    /// 判定条件が `uninstall` 固定だと、この方針へ変えた瞬間に補償制御だけが無音で消える。
    #[test]
    fn homebrew_cleanup_rejects_brew_below_floor_for_zap_mode() {
        let module = r#"      cleanup = "zap";"#;
        let lock = lock_fixture().replace(r#""ref": "6.0.13""#, r#""ref": "5.1.1""#);
        let err =
            assert_homebrew_cleanup_matches_locked_brew_capability(module, &lock).unwrap_err();
        assert!(err.to_string().contains("force-cleanup"), "{err}");
    }

    /// `zap` でも下限以上の brew なら受理する（下限判定が `uninstall` 専用になっていないことの対）。
    #[test]
    fn homebrew_cleanup_accepts_zap_mode_with_force_cleanup_capable_brew() {
        let module = r#"      cleanup = "zap";"#;
        assert!(
            assert_homebrew_cleanup_matches_locked_brew_capability(module, lock_fixture()).is_ok()
        );
    }

    /// 識別済みの旧 brew 向け迂回（`cleanup = "none"` + `extraFlags = [ "--cleanup" ]`）だけは下限を要求しない。
    #[test]
    fn homebrew_cleanup_skips_floor_for_identified_pre_force_cleanup_workaround() {
        let module = r#"      cleanup = "none";
      extraFlags = [ "--cleanup" ];"#;
        let lock = lock_fixture().replace(r#""ref": "6.0.13""#, r#""ref": "5.1.1""#);
        assert!(assert_homebrew_cleanup_matches_locked_brew_capability(module, &lock).is_ok());
    }

    /// `cleanup = "none"` 単体は迂回形として識別できないため、下限を黙って skip せず fail-closed にする。
    #[test]
    fn homebrew_cleanup_rejects_none_mode_without_cleanup_flag() {
        let module = r#"      cleanup = "none";"#;
        let err = assert_homebrew_cleanup_matches_locked_brew_capability(module, lock_fixture())
            .unwrap_err();
        assert!(err.to_string().contains("識別できない"), "{err}");
    }

    /// enum に存在するが brew 依存を確認していない方針（`check`）も fail-closed にする。
    #[test]
    fn homebrew_cleanup_rejects_unverified_cleanup_mode() {
        let module = r#"      cleanup = "check";"#;
        let err = assert_homebrew_cleanup_matches_locked_brew_capability(module, lock_fixture())
            .unwrap_err();
        assert!(err.to_string().contains("識別できない"), "{err}");
    }

    /// 宣言形が変わってアンカーが外れた状態（0 件）は、guard を dormant にせず fail-closed にする。
    #[test]
    fn homebrew_cleanup_rejects_module_without_cleanup_declaration() {
        let module = r#"      onActivation.cleanup = cleanupMode;"#;
        let err = assert_homebrew_cleanup_matches_locked_brew_capability(module, lock_fixture())
            .unwrap_err();
        assert!(err.to_string().contains("1 件に確定できない"), "{err}");
    }

    /// 宣言が複数見つかる状態も、どれを検査対象にすべきか確定できないため fail-closed にする。
    #[test]
    fn homebrew_cleanup_rejects_ambiguous_cleanup_declarations() {
        let module = r#"      cleanup = "uninstall";
      cleanup = "none";"#;
        let err = assert_homebrew_cleanup_matches_locked_brew_capability(module, lock_fixture())
            .unwrap_err();
        assert!(err.to_string().contains("1 件に確定できない"), "{err}");
    }

    /// コメント内の説明（旧 brew 迂回の記述）は宣言として数えない。数えると実宣言が確定できなくなる。
    #[test]
    fn homebrew_cleanup_ignores_cleanup_examples_in_comments() {
        let module = r#"      # 一時期 cleanup = "none" + extraFlags = [ "--cleanup" ] で迂回していた。
      cleanup = "uninstall";"#;
        assert!(
            assert_homebrew_cleanup_matches_locked_brew_capability(module, lock_fixture()).is_ok()
        );
    }

    /// brew tag が x.y.z として解釈できない形式へ変わったら、黙って pass させず fail-closed にする。
    #[test]
    fn homebrew_cleanup_rejects_unparsable_brew_reference() {
        let module = r#"      cleanup = "uninstall";"#;
        let lock = lock_fixture().replace(r#""ref": "6.0.13""#, r#""ref": "master""#);
        let err =
            assert_homebrew_cleanup_matches_locked_brew_capability(module, &lock).unwrap_err();
        assert!(err.to_string().contains("x.y.z"), "{err}");
    }

    /// `brew-src` の `original.ref` 自体が消えた（nix-homebrew の input 構成変化）場合も fail-closed にする。
    #[test]
    fn homebrew_cleanup_rejects_lock_without_brew_reference() {
        let module = r#"      cleanup = "uninstall";"#;
        let lock = lock_fixture().replace(r#""ref": "6.0.13", "#, "");
        let err =
            assert_homebrew_cleanup_matches_locked_brew_capability(module, &lock).unwrap_err();
        assert!(err.to_string().contains("original.ref"), "{err}");
    }

    /// 版比較は 3 要素の `x.y.z` に限る。要素数が違う tag を既定値扱いで通すと下限判定が骨抜きになる。
    #[test]
    fn dotted_version_rejects_reference_without_three_components() {
        assert!(parse_dotted_version("6.0.13").is_some());
        assert!(parse_dotted_version("6.0").is_none());
        assert!(parse_dotted_version("6.0.13.1").is_none());
    }

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
            env:
              REPO_BASE_SHA: ${{ needs.bump.outputs.repo_base_sha }}
            run: |
              nix develop -c "$dotfiles_bin" update-history record \
                --lock-old old-flake.lock \
                --lock-new flake.lock \
                --cursor-old "$REPO_BASE_SHA" \
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
}
