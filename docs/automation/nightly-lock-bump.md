# nightly 自動 bump とゲート

この文書は、nightly が `flake.lock` を無人 bump して auto-merge するまでの方針とゲートの正本である。

[`.github/workflows/nightly-update.yml`](../../.github/workflows/nightly-update.yml) が nightly に repo の
`flake.lock` を bump し、更新履歴を [`docs/update-history/<YYYY-MM>.toml`](../update-history/README.md) へ記録して
自動 PR を起票・auto-merge する。各マシンはこの bump 済み pin に `dotfiles update` で追随する。

## bump 対象

bump は `nix flake update` を引数なしで実行し、**`flake.lock` の全 input** を対象にする。framework input
（nix-darwin / home-manager / nix-homebrew）とその推移 input（nix-homebrew の `brew-src`）も含む。

一部 input を bump 対象から外すと、除外分だけが据え置かれたまま他が前進し、上流が検証していない組み合わせへ
収束する。除外に対応する有人 bump 経路も無いため、除外は実質「更新されない」と同義になる。

## auto-merge を止めるゲート

GITHUB_TOKEN が起票・push した PR では GitHub が `on: pull_request` の workflow を発火しないため、required
check（`static-checks.yml` の job 名 `static checks`）を満たす commit status は nightly の open-pr job が自分で
投稿する。投稿は次のすべてが同一 run 内で合格した場合に限る（fail-closed）。

| ゲート | 実行 job | 検査しないもの |
|---|---|---|
| `cargo xtask ci verify-bump-lock` | open-pr | lock 内容の妥当性（許可パスと取得先同一性だけを見る） |
| `cargo xtask check static`（`darwinConfigurations.ci-ref` のモジュール評価を含む） | open-pr | derivation の build（devShell 内で走るため build sandbox でだけ露見する依存欠落） |
| `nix build .#dotfiles-cli` | open-pr | checkPhase だけが起動する実行時依存（derivation は `doCheck = false`） |
| 構成適用後の `bats tests/zsh`（`static-checks.yml` と同一） | runtime-gate | macOS 固有の挙動（runner は ubuntu） |
| `nix eval .#darwinConfigurations.ci-ref...`（`check static` と同じ評価を bump 直後に行う） | bump | 実機 activation |

open-pr は `needs: [bump, runtime-gate]` なので、bump job の eval か runtime-gate の実挙動 gate が fail した夜も
status は投稿されない。

構成適用と `bats tests/zsh` を open-pr ではなく runtime-gate に置くのは、権限の分離のためである。open-pr は
PR の push と status 投稿のために `contents` / `pull-requests` / `statuses` を write で持ち、`Nix を導入` step で
`cachix/install-nix-action` へ `github_access_token` を渡す。bump 後 lock の Nix コードを実行する gate は、
`contents: read` だけを持つ runtime-gate 側で走らせる。

Home Manager の activation は runner 上で実行するが、darwin activation（`darwin-rebuild switch` / `brew bundle`）
は実行しない。darwin activation を実行するのは
[`runtime-integration.yml`](../../.github/workflows/runtime-integration.yml)（週次 cron / `workflow_dispatch`）と
各マシンの `dotfiles update` である。

## 取得先期待値表の保守義務

`verify-bump-lock` は取得先期待値表（[`rust/xtask/src/ci/bump_lock.rs`](../../rust/xtask/src/ci/bump_lock.rs) の
`EXPECTED_LOCK_INPUT_SOURCES`）に期待値を持たない node の `locked` が動くと fail する。

`flake.nix` に input を追加・削除・rename したら、同じ差分で `EXPECTED_LOCK_INPUT_SOURCES` も更新する。更新漏れは
当該 PR の時点では止まらない。その input が bump された翌晩に `verify-bump-lock` が
`has no expected source identity entry` で fail し、nightly PR が毎晩失敗する形で現れる。

上流 flake が自身の input を追加・削除・rename した夜は、`verify-bump-lock` の node 集合一致検査が
`flake.lock node set changed` で fail する。bump ブランチは push 前に fail するため remote に残らない。失敗 run の
log の `added:` / `removed:` と、bump job の artifact `bump-state`（`retention-days: 1`）に含まれる bump 後 lock を、
既定ブランチの `flake.lock`（bump 前 lock。同 job の `repo_base_sha` output が指す commit のもの）と比較する。
変化が上流宣言どおりで root input（`flake.nix` 直下）が動いていなければ正当な上流変化であり、
`EXPECTED_LOCK_INPUT_SOURCES` を通常の PR で更新する。このとき artifact の bump 後 `flake.lock` も同じ PR へ
取り込む。root input の `original` が動いている、または追加
node の取得先が上流宣言と一致しない場合は攻撃を疑って調査する。

## インライン `verify-bump-lock` の判定内容と適用範囲

判定規則と反例は [`rust/xtask/src/ci/bump_lock.rs`](../../rust/xtask/src/ci/bump_lock.rs) の module doc と
unit test を正本とする。

この検査は nightly workflow の open-pr job が同一 run 内で実行するため、nightly が自分で起票した bump PR にのみ
適用される。第三者が同じ branch prefix（`nightly/bump-*`）で直接起票した PR には走らない。

## auto-merge の成立条件

合格後、open-pr job は `@codex review` コメントで codex 自動レビューを起動し（Copilot は GitHub 側のネイティブ
code review で走る）、`gh pr merge --auto --squash` で auto-merge を有効化する。`--auto` は ruleset の required
requirement が揃った時点でマージするため、マージ条件は「main」ruleset が強制するものに一致する。すなわち
required status `static checks`（`strict_required_status_checks_policy` により base 追随も必須）、code scanning
（CodeQL）、code quality、および未解決 review thread が無いこと（`required_review_thread_resolution`）であり、
必要承認数は 0 である。`non_fast_forward` は force-push を止める branch 規則でマージ条件ではなく、実マージ形態を
縛るのは `allowed_merge_methods: ["squash"]` である。

AI レビューの完了はこの条件に含まれない。ruleset の `copilot_code_review` は「push で Copilot レビューを
起票する」規則であってレビュー完了を待たせず、codex は required check でも required reviewer でもない。
Copilot / codex が指摘を review thread として投稿した後であれば `required_review_thread_resolution` が未解決
スレッドの残る PR のマージを止めるが、投稿前に上記 requirement が揃えば auto-merge はレビュー応答を待たずに
成立する。
