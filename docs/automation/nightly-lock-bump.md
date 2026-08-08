# nightly 自動 bump とゲート

この文書は、nightly が `flake.lock` を無人 bump して auto-merge するまでの方針と threat model の正本である。

[`.github/workflows/nightly-update.yml`](../../.github/workflows/nightly-update.yml) が nightly に repo の
`flake.lock` を bump し、更新履歴を [`docs/update-history/<YYYY-MM>.toml`](../update-history/README.md) へ記録して
自動 PR を起票・auto-merge する。各マシンはこの bump 済み pin に `dotfiles update` で追随する。

## bump 対象

bump は `nix flake update` を引数なしで実行し、**`flake.lock` の全 input** を対象にする。framework input
（nix-darwin / home-manager / nix-homebrew）とその推移 input（nix-homebrew の `brew-src`）も含む。

一部 input を bump 対象から外すと、除外分だけが据え置かれたまま他が前進し、上流が検証していない組み合わせへ
収束する。実際に `brew-src` が `5.1.1` に据え置かれたまま `homebrew-cask` だけ nightly で前進した結果、現行 cask の
`depends_on :macos`（位置引数）を旧 brew の `def depends_on(**kwargs)` が解釈できず、`dotfiles update` の
`brew bundle` 段が停止した。除外に対応する有人 bump 経路も存在しなかったため、除外は実質「更新されない」と
同義だった。

列挙形式への退行は `cargo xtask check static` の静的検査（[`rust/tests/checks/src/static_checks.rs`](../../rust/tests/checks/src/static_checks.rs)
の `nightly_bump_updates_every_input`）が止める。

## auto-merge を止めるゲート

無人 bump された PR は、open-pr job が同一 run 内で `cargo xtask ci verify-bump-lock`、`cargo xtask check static`、
`nix build .#dotfiles-cli` を実行し、3 つすべてが合格した場合に限り `static checks` status を投稿する。open-pr は
`needs: [bump, record]` なので、record job の `nix eval .#darwinConfigurations.ci-ref...`（nix-darwin +
home-manager のモジュール評価）が fail した夜も status は投稿されず、required check 未充足で auto-merge は
成立しない。

これらが実行しないのは **実機 activation**（`darwin-rebuild switch` / `brew bundle`）である。本変更の動機と
なった障害はまさにこのクラスで、eval でも `nix flake check` でも `nix build` でも検知できない。受容条件は
「残留制約」節に置く。

## 取得先期待値表の保守義務

`verify-bump-lock` は取得先期待値表（[`rust/xtask/src/ci/bump_lock.rs`](../../rust/xtask/src/ci/bump_lock.rs) の
`EXPECTED_LOCK_INPUT_SOURCES`）に期待値を持たない node の `locked` が動くと fail する。全 input bump により、
この表の責務は「どの input を bump してよいか」ではなく **各 node の期待 owner/repo（取得先同一性）** であり、
内容は実在 input の写しである。

`flake.nix` に input を追加・削除・rename したら、同じ差分で `EXPECTED_LOCK_INPUT_SOURCES` も更新する。更新漏れは
`cargo xtask check static` の静的検査（[`rust/tests/checks/src/static_checks.rs`](../../rust/tests/checks/src/static_checks.rs)
の `nightly_lock_input_sources_match_expected_table`）が実 `flake.lock` と突合して当該 PR の時点で止める。

上流 flake が自身の input を追加・削除・rename した夜は、`verify-bump-lock` の node 集合一致検査が
`flake.lock node set changed` で fail する。bump ブランチは push 前に fail するため remote に残らない。失敗 run の
log の `added:` / `removed:` と、bump job の artifact `bump-state`（`retention-days: 1`）に含まれる bump 前後の
lock を比較し、変化が上流宣言どおりで root input（`flake.nix` 直下）が動いていなければ正当な上流変化である。その
場合は `EXPECTED_LOCK_INPUT_SOURCES` を通常の PR で更新する。root input の `original` が動いている、または追加
node の取得先が上流宣言と一致しない場合は攻撃を疑って調査する。

## 更新概要（change_items）の取得

各アプリの「何が変わったか」概要は上流のリリースノートから取得するが、ノートの置き場は一律に機械取得
できないため、(1) 機械的に取れるものは Releases API / changelog から取得し、(2) 取れないものは GitHub
Models ではなく OpenAI API（`async-openai` crate）の AI エージェントに探させる。概要取得は nightly の GitHub secret
`OPEN_AI_API_KEY` を要し、未設定（ローカル等）やノートが取れない場合はそのパッケージを version-only
（version old→new + notes_url のみ）としてその場で確定記録する（1 回の record で全変更パッケージを処理し
きり、夜をまたいで埋め直さない）。さらに、**どこからノートを取得したか（provenance）を
`docs/update-history/notes-sources.toml`（ノート取得元レジストリ）に保存**し、次回以降の record はこの
レジストリを最優先参照して同じ取得元を再利用する（再探索しない）。これにより AI 探索は新規/未知パッケージと
自己修復（取得元が移動した等）に限定され、回を追って OpenAI API の呼び出しが逓減する。レジストリはパッケージ名
昇順の決定論ソートで diff を最小化し、`docs/update-history/**` 配下にあるため nightly の commit 許可パス内で
repo に入る。AI 由来の取得元 URL も含め、保存・再取得時は必ず許可ホスト https に限定して検証する。

## PR 起票と required check

PR 起票・auto-merge は **`GITHUB_TOKEN`（`github.token`）で完結**する。別途 GitHub App を作って secret を
仕込む必要はない。GITHUB_TOKEN が起票/push した PR では GitHub が `on: pull_request` の workflow
（必須 check）を発火しない既知の制約があるため、`nightly-update.yml` の open-pr job が **同一 run 内で
`cargo xtask ci verify-bump-lock`、`cargo xtask check static`、`nix build .#dotfiles-cli` を実行**し
（後 2 者は `static-checks.yml` と同一コマンド）、**3 つすべてが合格した場合に限り** PR head commit へ
`static checks` という commit status を投稿して required check を満たす（「auto-merge を止めるゲート」節）。
`static checks` は
[`.github/workflows/static-checks.yml`](../../.github/workflows/static-checks.yml) の job 名であり、適用済み
「main」ruleset の required context と context 名で突合する。

## インライン `verify-bump-lock` の判定内容

インラインのセキュリティチェックは、PR の base..head 全 commit 履歴に対して次を機械判定する。

- 変更パスが `flake.lock` と `docs/update-history/**` だけであること（`.github/**`・ソースが混ざれば fail）。
- `flake.lock` 差分が、取得先期待値表に期待値を持つ node（現行 `flake.nix` の root input 9 本と推移 input
  `brew-src`。表の維持義務は「取得先期待値表の保守義務」節）の rev 変更
  だけで、想定外 input の追加・削除、期待値を持たない node の rev 変更、source 改変が無いこと。owner/repo は
  厳密一致で照合し、`type` / `url` / `host` / `dir` / node 間 wiring / `flake` フラグの drift も fail にする。
  加えて、期待値が一致する node でも rev が変わらないまま `narHash` / `lastModified` だけが動く（同一 rev の
  取得物すり替え＝content swap）変更は fail にする。
- 唯一の緩和は**推移 input の `ref`** である。親 flake（nix-homebrew）を bump すると親側の宣言が動くため、
  `brew-src` の `original.ref` は `5.1.1` → `6.0.13` のように動く。この 1 フィールドの差分だけを推移
  input に許可し、owner/repo など取得先そのものを決めるフィールドの差分は許可しない。緩和は**方向を問わず**
  無条件であり、`ref` の前進・後退も親 bump の有無も判定しない（後退方向の実害を止めるのは「残留制約」節の
  静的検査である）。root input の `original` は本 repo の `flake.nix` 由来であり、nightly PR は `flake.nix` を
  変更できない（許可パス外）ため完全一致を要求する。

判定規則の詳細と反例は [`rust/xtask/src/ci/bump_lock.rs`](../../rust/xtask/src/ci/bump_lock.rs) の module
doc と unit test を正本とする。

## 検査者と検査対象の分離

このチェックは **検査者と検査対象を分離**する。判定バイナリ（`cargo xtask ci verify-bump-lock`）は nightly
workflow 自身の信頼 ref の checkout からビルドし、検査対象は base..head の lock 差分（git データ）である。PR 作業
ツリーの dotfiles を検査主体にしないため、悪意ある lock 改変があっても判定主体は信頼コードのままである。判定
ロジックは Rust の純粋核（[`rust/xtask/src/ci/bump_lock.rs`](../../rust/xtask/src/ci/bump_lock.rs)）に置き、
unit test で固定している。open-pr job のいずれかのゲートが fail すると `static checks` status は
投稿されず、required check が満たされないため無人 auto-merge は成立しない（fail-closed・人手レビュー経路へ
送られる）。許可パスが
`flake.lock` + `docs/update-history/**` に限定されるため、nightly PR が `.github/**`（workflow/guard）を
変更しようとしてもこのチェックで fail する。

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

ゲートはいずれも実機 activation（`darwin-rebuild switch` / `brew bundle`）を実行しないため、framework input の
無人 bump が activation 経路だけを壊す変更（本変更の動機となった brew の `--force-cleanup` 非互換がこれにあたる）
は auto-merge 前には検出できない。検出経路は
[`runtime-integration.yml`](../../.github/workflows/runtime-integration.yml)（週次 cron / `workflow_dispatch`）と
各マシンの `dotfiles update` 実行時であり、無人 merge から検出までの遅延は最大 1 週間になる。この残留リスクを
明示受容する。復旧は `flake.lock` の revert または該当モジュール側の迂回で行い、いずれも nightly の許可パス外
なので通常の PR 経路を通る。

既知の依存は個別に機械化する。[`nix/modules/homebrew.nix`](../../nix/modules/homebrew.nix) の
`cleanup = "uninstall"` は brew 側の `--force-cleanup` に依存するため、`cargo xtask check static` の
`homebrew_cleanup_matches_locked_brew_capability` が `flake.lock` の `brew-src` の `ref` 下限を強制する。
`verify-bump-lock` は推移 input の `ref` 差分を方向を問わず通すので、下限を割る方向の bump を止めるのはこの
静的検査である。

## インライン `verify-bump-lock` の適用範囲（threat-model）

インラインの `cargo xtask ci verify-bump-lock` は、**nightly workflow が自分で起票する bump PR にのみ適用**される
（open-pr job が同一 run 内で実行するため）。第三者が `nightly/bump-*` prefix で**直接起票した PR には
`verify-bump-lock` は走らない**（全 PR を横断検査していた `nightly-bump-guard.yml` の required check は App 廃止に
伴い削除済み）。そうした攻撃者起票 PR のマージ阻止は、bypass 不能な「main」ruleset の必須 `static checks` と
Copilot/Codex の自動レビューに依存する（加えて `.github/**` の改変はそもそも許可パス外として弾かれる）。より
強い保護が必要なら、App 不要の per-PR guard workflow（`on: pull_request` で nightly-prefix PR に `verify-bump-lock`
を実行し、その結果を「main」ruleset の required check に加える）を別途有効化できる。本仕様は App / secret 不要を
維持するため、この per-PR guard は既定では有効化していない。
