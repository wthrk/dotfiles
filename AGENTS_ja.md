# AGENTS_ja.md

## プロジェクト概要

このリポジトリは、macOS のユーザー環境向け dotfiles を管理する Nix flake プロジェクトです。`dotfiles` CLI、Home Manager / nix-darwin モジュール、ローカル flake 用ヘルパーを提供します。

主な領域:

- Rust ワークスペース: `rust/dotfiles-cli`、`rust/dotfiles-core`、`rust/xtask`、および `rust/tests/` 配下の検証 crate。
- Nix flake とモジュール: `flake.nix`、`nix/home.nix`、`nix/darwin.nix`、`nix/modules/`。
- ユーザー設定: `config/` 配下の zsh と Neovim 設定。
- bootstrap の入口: `scripts/bootstrap.sh`。

秘密情報復旧基盤の作業を継続するときは、`docs/secret-recovery/tasks.md` と GitHub issue `#11` を進捗管理の入口にしてください。

## 翻訳同期

- `AGENTS_ja.md` は `AGENTS.md` の正確な日本語訳でなければなりません。
- `AGENTS.md` を編集するときは、同じ変更で `AGENTS_ja.md` も更新してください。
- レビュー時は、`AGENTS_ja.md` が `AGENTS.md` と意味的に等価なままか確認してください。

## セットアップ

リポジトリ作業には flake の dev shell を使ってください:

```sh
direnv allow .
```

`direnv` が有効でない場合:

```sh
nix develop
```

リポジトリ外の生成済み dotfiles やマシン固有 dotfiles を手編集しないでください。正となる情報源は、このリポジトリと `~/.config/dotfiles` 配下に生成されるローカル flake です。

開発者の実 `~/.config/dotfiles` に書き込んで、ローカル flake 生成を手動テストしないでください。ローカル flake 生成はリポジトリのサンドボックス化された検証で確認し、特に新規マシン相当の挙動や対象パスでの挙動は runtime tests で確認してください。

## ブランチ

- 作業を始める前に、必ず現在のブランチを確認し、依頼された変更に適したブランチか判断してください。
- 現在のブランチが適切でない場合は、ファイルを編集する前に適切な作業ブランチを作成するか、適切な作業ブランチへ切り替えてください。
- 新しい作業ブランチを作成するときは常に、まず `main` が upstream に対して最新か確認し、必要なら更新してから、最新の `main` から分岐してください。
- ローカルの `main` が最新だと仮定しないでください。`main` を分岐元に使う前に upstream の状態を fetch してください。

## 開発コマンド

リポジトリ保守には `xtask` を使ってください:

```sh
cargo xtask check
cargo xtask apply
cargo xtask apply home-manager
```

必要に応じて、ワークスペースから公開 CLI を実行します:

```sh
cargo run --package dotfiles-cli -- init
cargo run --package dotfiles-cli -- switch home
cargo run --package dotfiles-cli -- switch darwin
cargo run --package dotfiles-cli -- switch all
```

## テスト

通常の変更を終える前に、既定の検証 suite を実行してください:

```sh
cargo xtask check
```

検証コマンドは常に flake dev shell から実行してください。

既定の検証は静的検証のみを実行します。zsh の起動・挙動検証や、Tart VM を使う runtime integration は実行しません。

個別確認:

```sh
cargo xtask check static
cargo xtask check zsh
```

zsh 設定、shell startup behavior、TAB bindings、fzf-tab、autosuggestions、syntax highlighting、PATH handling に影響する変更では、個別の zsh 検証を実行してください。

bootstrap、初回実行、ホスト切り替え、マシンをまたぐ前提に影響する変更では runtime checks を使ってください:

```sh
cargo xtask check runtime
```

runtime integration を含めてすべて実行する場合:

```sh
cargo xtask check all
```

Runtime checks には、dev shell に含まれる Tart/Packer/Ansible などの Darwin VM 用ツールが必要です。静的検証の詳細は `rust/tests/checks/src/static_checks.rs`、zsh の不変条件は `rust/tests/checks/src/zsh.rs` にあります。

## コードスタイル

コメント:

- 非自明なモジュール、スクリプト、コマンド入口、検証フローを定義するファイルには、そのファイルの役割を説明するファイル冒頭コメントまたは言語標準のドキュメントコメントが必要です。
- リポジトリ由来の説明コメントは、既存の Rust、Nix、shell のコメントスタイルに合わせて日本語で書いてください。周囲のファイルがすでに英語で書かれている場合、upstream からコピーしたテキストの場合、または外部形式が要求する場合だけ英語を使ってください。
- コメントは、永続的な設計意図、不変条件、制約、または非自明な運用上の文脈を説明しなければなりません。コードの言い換え、個人的な作業メモ、曖昧な TODO/FIXME を書かないでください。
- 挙動を変更するときは、同じ patch で近くのコメントも更新してください。誤解を招くコメントは、古い履歴として inline に残さず削除してください。

Rust:

- ワークスペースの edition は Rust 2024 です。
- 公開 CLI のロジックは `rust/dotfiles-cli`、リポジトリ保守コマンドは `rust/xtask`、共通ヘルパーは `rust/dotfiles-core` に置いてください。
- リポジトリの result alias を通じて `anyhow` を使い、panic ではなく context を付けて伝播してください。
- warnings を clean に保ってください。check suite は warnings を errors として扱います。

Nix:

- ユーザー設定は Home Manager モジュール、ホスト・システム設定は nix-darwin モジュールに置いてください。
- 明示的に breaking migration を求められていない限り、公開 flake API を維持してください。
- 再利用可能なモジュールに具体的なユーザー名、ホスト名、マシン固有パスを入れないでください。これらは `dotfiles.user`、`dotfiles.host`、または生成されたローカル flake から渡さなければなりません。
- Nix は `cargo xtask check static` が使う flake formatter で format してください。

Shell/zsh:

- `scripts/bootstrap.sh` は install-critical として扱ってください。portable に保ち、`bash -n` で syntax-check できるようにしてください。
- zsh の挙動は `rust/tests/checks/src/zsh.rs` と互換に保ってください。特に TAB bindings、fzf-tab placement、autosuggestions、syntax highlighting、legacy language managers の PATH exclusions を維持してください。
- app-managed shell injection や Docker authentication などのユーザー固有 runtime state は、このリポジトリ外に属します。

Lua/Neovim:

- 設定は `config/nvim/lua/omy/` 配下で、既存の `configs`、`mappings`、`autocmds` 構造に沿って整理してください。
- Neovim layout を作り替えるより、小さな module-local changes を優先してください。

## セキュリティ

- マシン固有の秘密情報、認証情報、API tokens、Docker auth state、SSH private keys、app session files を commit しないでください。
- 意図的に declarative にする場合を除き、mutable なマシン固有 state を Home Manager モジュールに入れないでください。
- Homebrew taps は flake inputs で pin されています。リポジトリ設計を明示的に変更しない限り、mutable tap behavior は避けてください。

## コミット規約

- コミット関連作業は sub-agent に委譲し、指示は次だけにしてください: `AGENTS.md` を読んで commit 作業を行う。sub-agent に rules や repository state を要約して渡さないでください。
- Conventional Commits を使ってください: `<type>(<scope>): <description>`。
- `feat`、`fix`、`docs`、`refactor`、`test`、`chore`、`build` などの common types を使ってください。
- description は日本語で書いてください。type と optional scope は ASCII のままにします。例: `docs: エージェント向け作業規約を追加`。
- commit messages を作業ログ、agent notes、chat summaries として書かないでください。
- 各 commit は 1 つの logical change に集中してください。review、revert、explain を独立して行える変更は commit を分割してください。
- behavior changes と mechanical formatting、documentation updates と code changes、generated output と source changes、refactors と functional fixes は分けてください。
- 分割すると intermediate commit が broken、misleading、または required tests/docs を欠く場合だけ、変更をまとめてください。
- PR grouping と commit grouping は別の判断です。ユーザーが明示的に 1 commit を要求しない限り、1 PR は 1 commit を意味しません。
- commit boundaries や messages を判断するときは、`git status` と `git diff` を確認してください。memory、chat context、assumptions に頼らないでください。
- runtime checks を skip した、または実行できなかった場合など、重要なときは commit body に validation を記載してください。

## Pull Request 規約

- PR 関連作業は sub-agent に委譲し、指示は次だけにしてください: `AGENTS.md` を読んで PR 作業を行う。sub-agent に rules や repository state を要約して渡さないでください。
- `main` に直接 push しないでください。すべての repository changes は feature branch と PR を通してください。
- Branch names は Conventional Commits と同じ type vocabulary を使い、`<type>/<scope>-<short-kebab-description>` としてください。lowercase にし、personal notes や chat context を含めないでください。
- PRs には、永続的な repository change、なぜ必要か、実施した validation を説明してください。chat history や agent workflow を書かないでください。
- PRs は focused にしてください。1 つの PR に複数の logical commits を含めてもよく、review に役立つ場合はその構造を要約してください。
- PR scope、title、description を判断するときは、`git status`、`git diff`、必要に応じて staged changes を確認してください。conversation だけから PR content を推測しないでください。
- user-visible behavior changes、bootstrap changes、module API changes、generated output removal、migration steps は明示してください。
- screenshots や command output summaries は review を明確にする場合だけ含めてください。noisy logs を貼らないでください。
- user-visible commands、bootstrap behavior、module boundaries、zsh key behavior を変更するときは `README.md` を更新してください。
- 期待される check を skip、blocked、または dev shell 外で実行した場合は、PR に明記してください。
