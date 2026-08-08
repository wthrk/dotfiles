//! VM を使わずに実行できる静的検証。
//!
//! Rust、shell script、Nix flake などの外部検証コマンドを順に実行する。

use std::collections::BTreeMap;

use anyhow::{anyhow, bail, ensure};
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
    nightly_lock_input_sources_match_expected_table(&shell)?;
    homebrew_cleanup_matches_locked_brew_capability(&shell)?;
    nix_diagnostics(&shell)?;
    nix(&shell)?;
    auto_update_daemon_drops_root_privileges(&shell)
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
    nightly_bump_updates_every_input(shell)?;
    Ok(())
}

/// nightly-update.yml の bump step が input を列挙せず、`nix flake update`（引数なし）で全 input を bump する
/// ことを静的に固定する。
///
/// input を列挙して一部を除外すると、除外分だけが据え置かれたまま他が前進し、上流が検証していない組み合わせへ
/// 収束する。除外に対応する有人 bump 経路も無いため、除外は「更新されない」と同義になる。
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
/// 表の更新を忘れると、その input が bump された翌晩に `verify-bump-lock` が fail し、nightly PR が毎晩
/// 失敗して auto-merge が恒久停止する。手書き定数と実 lock の drift を PR の時点で止める。
///
/// この検査が機械化するのは表の**網羅性**だけである。取得先の同一性そのものを強制するのは
/// `verify-bump-lock` 側である。
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

/// root LaunchDaemon（auto-update）が起動する wrapper を評価値から引くための daemon label。
const AUTO_UPDATE_DAEMON_LABEL: &str = "org.dotfiles.auto-update";

/// nix-darwin が `brew bundle` へ `--force-cleanup` を生成する `onActivation.cleanup` の値の集合。
///
/// nix-darwin の `modules/homebrew.nix` は `optional (cleanup == "uninstall") "--force-cleanup"` と
/// `optional (cleanup == "zap") "--zap --force-cleanup"` の 2 分岐でこのフラグを足す。したがって brew 版の
/// 下限は `uninstall` だけでなく `zap` にも掛かり、判定条件は個別の値ではなくこの集合で表す。
const CLEANUP_MODES_REQUIRING_FORCE_CLEANUP: [&str; 2] = ["uninstall", "zap"];

/// `--force-cleanup` を生成しない cleanup 方針として識別済みの値。
///
/// 現行 enum（`none` / `check` / `uninstall` / `zap`）のうち、brew 版下限と無関係だと確認済みなのは
/// `cleanup = "none"` + `extraFlags = [ "--cleanup" ]` の形だけである。それ以外の値は「brew のどの
/// capability に依存するか未確認」として fail-closed にする。
const CLEANUP_MODE_WITHOUT_FORCE_CLEANUP: &str = "none";

/// `homebrew.nix` の cleanup 方針と lock 済み brew の版が両立していることを静的に固定する。
///
/// `verify-bump-lock` は推移 input の `ref` 差分を**方向を問わず**通すため、lock 側の brew が
/// `--force-cleanup` を持たない版へ無人で戻っても guard は素通りし、switch 経路だけが実機で壊れる。
/// 「switch 経路が brew の版に依存している」という設計判断を lock 上の下限として固定するのが本検査であり、
/// 下限を割る必要が生じた場合は `homebrew.nix` の cleanup 方針と `BREW_REF_WITH_FORCE_CLEANUP` を同じ差分で
/// 更新する。
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
/// コメントを残したまま宣言を探すと、cleanup 方針を説明するコメントと実宣言を区別できない。`#` を含む
/// 文字列リテラルが同じ行に現れた場合は宣言が 1 件に確定しなくなり、`declared_homebrew_cleanup_mode` 側で
/// fail-closed になる。
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

/// root LaunchDaemon（auto-update）が起動する argv が、`dotfiles update` へ権限降格を渡すことを確認する。
///
/// この daemon は root で走る。argv から `--user` が落ちると Home Manager が root のまま実行され、利用者所有
/// ファイルの所有者が root へ変わる。この退行は評価も `nix flake check` も `nil` も通過するため、ここで固定
/// する。検査対象は `nix/darwin.nix` のソーステキストではなく `darwinConfigurations.ci-ref` の評価値であり、
/// wrapper は評価時に書き出された derivation から読むためビルドを要さない（darwin 以外の runner でも動く）。
fn auto_update_daemon_drops_root_privileges(shell: &Shell) -> Result<()> {
    step("auto-update daemon argv drops root privileges");
    let attribute = format!(
        ".#{CI_REFERENCE}.config.launchd.daemons.\"{AUTO_UPDATE_DAEMON_LABEL}\".serviceConfig.ProgramArguments"
    );
    // argv 要素の string context から、その argv が指す derivation を引く。
    let apply = "args: builtins.concatMap (arg: builtins.attrNames (builtins.getContext arg)) args";
    let referenced = cmd!(shell, "nix eval {attribute} --json --apply {apply}").read()?;
    let derivation = auto_update_wrapper_derivation(&referenced)?;
    let shown = cmd!(shell, "nix derivation show {derivation}").read()?;
    let script = auto_update_wrapper_script(&shown)?;
    assert_auto_update_daemon_drops_root_privileges(&script)
}

/// daemon の argv が参照する derivation を 1 件に確定する。
///
/// 0 件（argv が store 由来でない素のコマンドへ変わった）でも複数件でも、どれが root で実行される実体かを
/// 決められないため `Err` にする。ここを通すと権限降格の検査対象を黙って失う。
fn auto_update_wrapper_derivation(referenced: &str) -> Result<String> {
    let referenced: Vec<String> = serde_json::from_str(referenced)?;
    let [derivation] = &referenced[..] else {
        return Err(anyhow!(
            "auto-update daemon の ProgramArguments が参照する derivation を 1 件に確定できない\
             （検出 {} 件）。argv の組み立て方を変える場合は、root 実行される実体を特定して権限降格を \
             検査できる形に本検査も更新すること",
            referenced.len()
        ));
    };
    Ok(derivation.clone())
}

/// `nix derivation show` の出力から wrapper script 本文を取り出す。
///
/// 出力形（`derivations` で包むか否か）は nix の版で変わりうるため両方を受け、どちらとしても読めない場合や
/// `env.text` を持たない場合は `Err` にする。
fn auto_update_wrapper_script(shown: &str) -> Result<String> {
    let shown: serde_json::Value = serde_json::from_str(shown)?;
    let derivations = shown
        .get("derivations")
        .unwrap_or(&shown)
        .as_object()
        .ok_or_else(|| anyhow!("`nix derivation show` の出力を derivation の集合として読めない"))?;
    let [(_, derivation)] = derivations.iter().collect::<Vec<_>>()[..] else {
        return Err(anyhow!(
            "`nix derivation show` の出力に derivation が 1 件だけ含まれていない（検出 {} 件）",
            derivations.len()
        ));
    };
    derivation
        .get("env")
        .and_then(|env| env.get("text"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            anyhow!(
                "auto-update wrapper の derivation に script 本文（env.text）が無い。wrapper の生成手段を \
                 変える場合は、root LaunchDaemon の argv から権限降格を読み取れる形に本検査も更新すること"
            )
        })
}

/// wrapper script が起動する `dotfiles update` の argv に、root 以外への降格指定が含まれることを判定する純関数。
///
/// 行継続を畳んでから `dotfiles update` の起動行を 1 件に確定し、その argv の `--user` の値を見る。起動行が
/// 確定できない、`--user` が無い、値が空か `root` の場合はいずれも `Err`（fail-closed）。
fn assert_auto_update_daemon_drops_root_privileges(script: &str) -> Result<()> {
    let joined = script.replace("\\\n", " ");
    let invocations: Vec<&str> = joined
        .lines()
        .filter(|line| line.contains("/bin/dotfiles update"))
        .collect();
    let [invocation] = invocations[..] else {
        return Err(anyhow!(
            "auto-update wrapper から `dotfiles update` の起動行を 1 件に確定できない（検出 {} 件）",
            invocations.len()
        ));
    };
    let arguments: Vec<&str> = invocation.split_whitespace().collect();
    let user = arguments
        .iter()
        .enumerate()
        .find_map(|(index, argument)| {
            argument
                .strip_prefix("--user=")
                .or_else(|| (*argument == "--user").then(|| arguments.get(index + 1).copied())?)
        })
        .ok_or_else(|| {
            anyhow!(
                "root LaunchDaemon が起動する `dotfiles update` の argv に `--user` が無い。権限降格が \
                 落ちると Home Manager が root のまま走り、利用者所有ファイルが root 所有へ変わる"
            )
        })?;
    ensure!(
        !user.is_empty() && user != "root",
        "root LaunchDaemon が起動する `dotfiles update` の `--user` が `{user}` で、root からの降格に \
         なっていない"
    );
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

#[cfg(test)]
mod tests {
    use super::{
        assert_auto_update_daemon_drops_root_privileges,
        assert_homebrew_cleanup_matches_locked_brew_capability,
        assert_lock_input_sources_match_expected_table, assert_nightly_bump_updates_every_input,
        auto_update_wrapper_derivation, auto_update_wrapper_script, parse_dotted_version,
    };

    /// 評価済み wrapper script の骨格。`--user` の行（継続行込み、無い場合は空文字列）だけを差し替える。
    fn wrapper_fixture(user_line: &str) -> String {
        format!(
            "#!/nix/store/aaaa-bash/bin/bash\n\
             set -euo pipefail\n\n\
             export PATH=/nix/store/bbbb-nix/bin\n\n\
             exec env HOME=/Users/ci /nix/store/cccc-dotfiles-cli/bin/dotfiles update \\\n  \
             --config-dir /Users/ci/.config/dotfiles \\\n\
             {user_line}  --host ci-ref\n"
        )
    }

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

    /// root daemon の argv が対象ユーザーへ降格していれば受理する。
    #[test]
    fn auto_update_daemon_accepts_argv_with_user_downgrade() {
        let script = wrapper_fixture("  --user ci \\\n");
        assert!(assert_auto_update_daemon_drops_root_privileges(&script).is_ok());
    }

    /// `--user=<user>` 形式も同じ降格として受理する。
    #[test]
    fn auto_update_daemon_accepts_joined_user_option_form() {
        let script = wrapper_fixture("  --user=ci \\\n");
        assert!(assert_auto_update_daemon_drops_root_privileges(&script).is_ok());
    }

    /// `--user` が落ちると Home Manager が root のまま走るため拒否する。
    #[test]
    fn auto_update_daemon_rejects_argv_without_user_downgrade() {
        let script = wrapper_fixture("");
        let err = assert_auto_update_daemon_drops_root_privileges(&script).unwrap_err();
        assert!(err.to_string().contains("--user"), "{err}");
    }

    /// `--user root` は形の上では降格指定だが root のままなので拒否する。
    #[test]
    fn auto_update_daemon_rejects_root_as_downgrade_target() {
        let script = wrapper_fixture("  --user root \\\n");
        let err = assert_auto_update_daemon_drops_root_privileges(&script).unwrap_err();
        assert!(err.to_string().contains("root"), "{err}");
    }

    /// `dotfiles update` の起動行を確定できない argv は、検査対象を失うため拒否する。
    #[test]
    fn auto_update_daemon_rejects_script_without_update_invocation() {
        let err =
            assert_auto_update_daemon_drops_root_privileges("set -euo pipefail\n").unwrap_err();
        assert!(err.to_string().contains("1 件に確定できない"), "{err}");
    }

    /// argv が参照する derivation が 1 件なら、その path を検査対象として返す。
    #[test]
    fn wrapper_derivation_accepts_single_referenced_derivation() {
        let referenced = r#"["/nix/store/aaaa-org.dotfiles.auto-update-wrapper.drv"]"#;
        assert_eq!(
            auto_update_wrapper_derivation(referenced).unwrap_or_default(),
            "/nix/store/aaaa-org.dotfiles.auto-update-wrapper.drv"
        );
    }

    /// argv が store 由来の実体を 1 件に確定できない形（0 件・複数件）は fail-closed にする。
    #[test]
    fn wrapper_derivation_rejects_ambiguous_reference_set() {
        assert!(auto_update_wrapper_derivation("[]").is_err());
        assert!(auto_update_wrapper_derivation(r#"["/a.drv","/b.drv"]"#).is_err());
    }

    /// `nix derivation show` の出力から script 本文を取り出す。
    #[test]
    fn wrapper_script_reads_derivation_text() {
        let shown = r#"{"derivations":{"/nix/store/aaaa.drv":{"env":{"text":"exec dotfiles update"}}},"version":4}"#;
        assert_eq!(
            auto_update_wrapper_script(shown).unwrap_or_default(),
            "exec dotfiles update"
        );
    }

    /// script 本文を持たない derivation 形式へ変わったら、黙って pass させず fail-closed にする。
    #[test]
    fn wrapper_script_rejects_derivation_without_text() {
        let shown = r#"{"derivations":{"/nix/store/aaaa.drv":{"env":{}}},"version":4}"#;
        let err = auto_update_wrapper_script(shown).unwrap_err();
        assert!(err.to_string().contains("env.text"), "{err}");
    }
}
