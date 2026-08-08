# nightly 自動 bump とゲート

この文書は、nightly が `flake.lock` を無人 bump して auto-merge するまでの方針と threat model の正本である。

[`.github/workflows/nightly-update.yml`](../../.github/workflows/nightly-update.yml) が nightly に repo の
`flake.lock` を bump し、更新履歴を [`docs/update-history/<YYYY-MM>.toml`](../update-history/README.md) へ記録して
自動 PR を起票・auto-merge する。各マシンはこの bump 済み pin に `dotfiles update` で追随する。

## bump 対象

bump は `nix flake update` を引数なしで実行し、**`flake.lock` の全 input** を対象にする。framework input
（nix-darwin / home-manager / nix-homebrew）とその推移 input（nix-homebrew の `brew-src`）も含む。

一部 input を bump 対象から外すと、除外分だけが据え置かれたまま他が前進し、上流が検証していない組み合わせへ
収束する。除外に対応する有人 bump 経路も無いため、除外は実質「更新されない」と同義になる。列挙形式への退行は
`cargo xtask check static` の `nightly_bump_updates_every_input` が止める。

## auto-merge を止めるゲート

GITHUB_TOKEN が起票・push した PR では GitHub が `on: pull_request` の workflow を発火しないため、required
check（`static-checks.yml` の job 名 `static checks`）を満たす commit status は nightly の open-pr job が自分で
投稿する。投稿は次のすべてが同一 run 内で合格した場合に限る（fail-closed）。

| ゲート | 検査しないもの |
|---|---|
| `cargo xtask ci verify-bump-lock` | lock 内容の妥当性（許可パスと取得先同一性だけを見る） |
| `cargo xtask check static`（`darwinConfigurations.ci-ref` のモジュール評価を含む） | derivation の build（devShell 内で走るため build sandbox でだけ露見する依存欠落） |
| `nix build .#dotfiles-cli` | checkPhase だけが起動する実行時依存（derivation は `doCheck = false`） |
| bump job の `nix eval .#darwinConfigurations.ci-ref...`（`check static` と同じ評価を bump 直後に行う） | 実機 activation |

open-pr は `needs: bump` なので、最後の eval が fail した夜も status は投稿されない。

いずれも実機 activation（`darwin-rebuild switch` / `brew bundle`）は実行しない。受容条件は「残留制約」節に置く。

## 取得先期待値表の保守義務

`verify-bump-lock` は取得先期待値表（[`rust/xtask/src/ci/bump_lock.rs`](../../rust/xtask/src/ci/bump_lock.rs) の
`EXPECTED_LOCK_INPUT_SOURCES`）に期待値を持たない node の `locked` が動くと fail する。

`flake.nix` に input を追加・削除・rename したら、同じ差分で `EXPECTED_LOCK_INPUT_SOURCES` も更新する。更新漏れは
`cargo xtask check static` の `nightly_lock_input_sources_match_expected_table` が実 `flake.lock` と突合して当該
PR の時点で止める。

上流 flake が自身の input を追加・削除・rename した夜は、`verify-bump-lock` の node 集合一致検査が
`flake.lock node set changed` で fail する。bump ブランチは push 前に fail するため remote に残らない。失敗 run の
log の `added:` / `removed:` と、bump job の artifact `bump-state`（`retention-days: 1`）に含まれる bump 後 lock を、
既定ブランチの `flake.lock`（bump 前 lock。同 job の `repo_base_sha` output が指す commit のもの）と比較する。
変化が上流宣言どおりで root input（`flake.nix` 直下）が動いていなければ正当な上流変化であり、
`EXPECTED_LOCK_INPUT_SOURCES` を通常の PR で更新する。root input の `original` が動いている、または追加 node の
取得先が上流宣言と一致しない場合は攻撃を疑って調査する。

## 更新概要（change_items）の取得

概要は上流のリリースノートから取得するが、ノートの置き場は一律に機械取得できないため、機械的に取れるものは
Releases API / changelog から取得し、取れないものは OpenAI API（`async-openai` crate）の AI エージェントに
探させる。API キーが無い場合やノートが取れない場合は、そのパッケージを version-only（version old→new +
notes_url のみ）としてその場で確定記録する。

どこからノートを取得したか（provenance）は `docs/update-history/notes-sources.toml` に保存し、次回以降の record は
このレジストリを最優先参照して同じ取得元を再利用する。これにより AI 探索は新規/未知パッケージと自己修復
（取得元が移動した等）に限定される。AI 由来の取得元 URL も含め、保存・再取得時は許可ホスト https に限定して
検証する。

## インライン `verify-bump-lock` の判定内容

判定規則と反例は [`rust/xtask/src/ci/bump_lock.rs`](../../rust/xtask/src/ci/bump_lock.rs) の module doc と
unit test を正本とする。

## 検査者と検査対象の分離

このチェックは **検査者と検査対象を分離**する。判定バイナリ（`cargo xtask ci verify-bump-lock`）は nightly
workflow 自身の信頼 ref の checkout からビルドし、検査対象は base..head の lock 差分（git データ）である。PR 作業
ツリーの dotfiles を検査主体にしないため、悪意ある lock 改変があっても判定主体は信頼コードのままである。

合格後、open-pr job は `@codex review` コメントで codex 自動レビューを起動し（Copilot は GitHub 側のネイティブ
code review で走る）、`gh pr merge --auto --squash` で auto-merge を有効化する。Copilot/Codex のレビュー
充足と required status（`static checks`）満了でマージされる。

## 残留制約

### 実 GitHub でのみ最終確認できる前提

この App 不要フローには、実 GitHub でしか確定できない前提がある（マージ後に `workflow_dispatch`
`dry_run=false` で検証する）。

- GITHUB_TOKEN で POST した commit status `static checks` が、適用済み ruleset の required context と確実に
  名前突合してマージ条件を満たすか。
- `gh pr merge --auto` を GITHUB_TOKEN 権限（`pull-requests: write`）で有効化できるか（repo 設定で
  auto-merge が有効である必要がある）。
- `copilot_code_review` が bot / GITHUB_TOKEN 起票 PR で発火するか。

いずれも未充足ならマージは保留され、無人で main に入らない（人手レビューへ送られる）。

### 無人 bump の runtime 非互換（明示受容）

ゲートはいずれも実機 activation を実行しないため、framework input の無人 bump が activation 経路だけを壊す変更は
auto-merge 前には検出できない。検出経路は
[`runtime-integration.yml`](../../.github/workflows/runtime-integration.yml)（週次 cron / `workflow_dispatch`）と
各マシンの `dotfiles update` 実行時であり、無人 merge から検出までの遅延は最大 1 週間になる。この残留リスクを
明示受容する。復旧は `flake.lock` の revert または該当モジュール側の迂回で行い、いずれも nightly の許可パス外
なので通常の PR 経路を通る。

既知の依存は個別に機械化する。`verify-bump-lock` は推移 input の `ref` 差分を方向を問わず通すため、
[`nix/modules/homebrew.nix`](../../nix/modules/homebrew.nix) の `cleanup` が要求する brew 側 capability の下限は
`cargo xtask check static` の `homebrew_cleanup_matches_locked_brew_capability` が `flake.lock` 上で強制する。

## インライン `verify-bump-lock` の適用範囲（threat-model）

インラインの `cargo xtask ci verify-bump-lock` は、**nightly workflow が自分で起票する bump PR にのみ適用**される
（open-pr job が同一 run 内で実行するため）。第三者が `nightly/bump-*` prefix で**直接起票した PR には
`verify-bump-lock` は走らない**。そうした攻撃者起票 PR のマージ阻止は、bypass 不能な「main」ruleset の必須
`static checks` と Copilot/Codex の自動レビューに依存する（加えて `.github/**` の改変はそもそも許可パス外として
弾かれる）。より強い保護が必要なら、App 不要の per-PR guard workflow（`on: pull_request` で nightly-prefix PR に
`verify-bump-lock` を実行し、その結果を「main」ruleset の required check に加える）を別途有効化できる。本仕様は
App / secret 不要を維持するため、この per-PR guard は既定では有効化していない。
