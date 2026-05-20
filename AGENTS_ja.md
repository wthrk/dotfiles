# AGENTS_ja.md

## 重要な計画ゲート

このリポジトリで `計画依頼` を扱う場合、正本は `docs/secret-recovery/implementation-guidelines.md` だけである。計画・実装・レビュー段階の役割分担を含む計画の実装単位、レビュー循環、実装方針はこの文書に従う。

`計画依頼` では、他の文書より先に `docs/secret-recovery/implementation-guidelines.md` の計画依頼向け固定実装単位を定義した箇所を確認し、定義済み実装単位を再定義せずに参照する。

チャット内で別の計画手順を作成、言い換え、要約、置換してはならない。一般的な計画習慣や既定ワークフローで、このリポジトリ固有の正本を上書きしてはならない。
secret-recovery の計画、実装、確認、レビュー、後続対応を伴う作業では、メインエージェントはオーケストレーション専任とする。`docs/secret-recovery/implementation-guidelines.md` で実装担当またはレビュー担当に割り当てられた repo 探索、コマンド実行、ファイル確認、差分確認、検証、レビュー、成果物編集は、自分で行ってはならない。

## プロジェクト概要

このリポジトリは、macOS の利用者環境向け dotfiles を管理する Nix flake プロジェクトである。`dotfiles` CLI、Home Manager / nix-darwin モジュール、ローカル flake 補助を提供する。

主な構成:

- Rust ワークスペース: `rust/dotfiles-cli`、`rust/dotfiles-core`、`rust/xtask`、`rust/tests/` 配下の検証 crate
- Nix flake とモジュール: `flake.nix`、`nix/home.nix`、`nix/darwin.nix`、`nix/modules/`
- 利用者設定: `config/` 配下の zsh / Neovim 設定
- bootstrap 入口: `scripts/bootstrap.sh`

secret-recovery の継続作業では、まず `docs/README.md` を文書入口として参照し、そのうえで進捗管理入口を `docs/secret-recovery/tasks.md` に固定する。実装変更やレビューの前に `docs/secret-recovery/implementation-guidelines.md` を読み、固定実装単位、役割分担、実装方針を適用する。
タスク管理の解釈規則（`次のタスク` のような指示を含む）は `docs/docs-governance.md` を正本とし、secret-recovery では `docs/secret-recovery/tasks.md` を参照し、進捗変化のたびに同ファイルへ反映する。
文書参照は `docs/README.md` と `docs/secret-recovery/README.md` を入口にし、各 README と `docs/docs-governance.md` に記載された「何を書く・何を書かない・どれを参照する」の範囲定義に従う。

## 翻訳同期

- `AGENTS_ja.md` は `AGENTS.md` と意味的に一致させる。
- `AGENTS.md` を編集する場合は、同一変更で `AGENTS_ja.md` も更新する。
- レビュー時に両文書の意味一致を確認する。

## 文書指示の実行

- プロンプト実行時は、着手前に今回依頼へ適用される文書指示を抽出し、実行中に必ず適用し続ける。
- 文書指示に従っていない提案・編集・報告は行わず、指示間に衝突があれば作業前に利用者へ確認する。

## コミュニケーション

- 利用者が別言語を明示しない限り、日本語で応答する。
- コードレビュー指摘、PR 要約、検証メモは日本語で記述する。
- 技術識別子、コマンド名、ファイルパス、コミット種別、上流引用は必要に応じて原文を保持する。

## セットアップ

作業は flake の開発シェルで行う。

```sh
direnv allow .
```

`direnv` が有効でない場合:

```sh
nix develop
```

リポジトリ外の生成済み dotfiles やマシン固有 dotfiles を手編集しない。正本はこのリポジトリと `~/.config/dotfiles` に生成されるローカル flake である。

開発者の実 `~/.config/dotfiles` に書き込んでローカル flake 生成を手動検証してはならない。新規マシン相当や対象パス挙動は、リポジトリのサンドボックス検証、とくに実行時テストで確認する。

## ブランチ

- 作業前に現在ブランチを確認し、依頼内容に適切か判断する。
- 不適切な場合は編集前に適切な作業ブランチへ切り替えるか新規作成する。
- 新規作業ブランチ作成時は、`main` が upstream 最新であることを確認し、必要なら更新してから分岐する。
- ローカル `main` を最新だと仮定しない。分岐前に upstream を fetch する。

## 指示遵守

- 編集や広範検証の前に、適用される指示を特定し、その制約内で実装する。
- コード変更では、完了前に差分を コードスタイル規則で確認する。formatter、lint、test の成功を指示遵守の代替にしない。

## 開発コマンド

保守作業は `xtask` を使う。

```sh
cargo xtask check
cargo xtask apply
cargo xtask apply home-manager
```

必要時の公開 CLI 実行:

```sh
cargo run --package dotfiles-cli -- init
cargo run --package dotfiles-cli -- switch home
cargo run --package dotfiles-cli -- switch darwin
cargo run --package dotfiles-cli -- switch all
```

## テスト

変更対象に関係する検証だけを選ぶ。変更を検証しない広範チェックを機械的に実行しない。

Markdown のみ変更では、生成文書や記載コマンド検証に関係する場合、または利用者が明示要求した場合を除き、`cargo xtask check` と `cargo xtask check static` を実行しない。代わりに `git diff --check`、必要な表示確認、変更リンク先確認を行う。

コード、Nix、shell、workflow、bootstrap、生成物の変更では既定検証を実行する。

```sh
cargo xtask check
```

成功済み検証は、差分変更後・最終差分未網羅・利用者再実行要求のいずれかがある場合のみ再実行する。`git status` や `git diff` など読み取り専用確認は再実行理由にならない。

検証は必ず flake dev shell から実行する。dev shell 外なら `direnv exec .` 経由で実行する。

```sh
direnv exec . cargo test ...
direnv exec . cargo xtask check static
```

既定検証は静的検証のみで、zsh 起動挙動や Tart VM 実行時統合は含まない。

個別チェック:

```sh
cargo xtask check static
cargo xtask check zsh
```

zsh 設定、起動挙動、TAB バインド、fzf-tab、autosuggestions、syntax highlighting、PATH 処理 に影響する変更では `cargo xtask check zsh` を実行する。

bootstrap、初回実行、ホスト切替、マシン横断前提 に影響する変更では 実行時チェック を実行する。

```sh
cargo xtask check runtime
```

全チェック実行:

```sh
cargo xtask check all
```

実行時チェック には dev shell の Darwin VM ツール群（Tart/Packer/Ansible）が必要である。静的チェック詳細は `rust/tests/checks/src/static_checks.rs`、zsh 不変条件は `rust/tests/checks/src/zsh.rs` を参照する。

## コードスタイル

- 非自明な module、script、コマンド入口、検証フロー定義ファイルには、役割を説明する ファイルレベルのコメント または言語標準 doc コメント を付ける。
- リポジトリ由来の説明コメントは日本語で書く。周辺が英語、上流引用、外部形式要件の場合のみ英語を許可する。
- コメントは恒久的な意図、不変条件、制約、非自明な運用文脈を説明し、単なるコード言い換え・個人メモ・曖昧 TODO/FIXME を禁止する。
- コメントが必要なときは、ライフサイクル境界、外部契約、シグナル安全要件、ワイヤ形式規則、セキュリティ特性、利用者操作制約 のいずれかを具体化する。
- 関数・型・module の doc コメント は主契約を先頭で示し、条件や失敗時契約は後段で分離して記述する。
- 挙動変更時は近傍コメントを同パッチで更新し、誤解を生む旧コメントは残さない。

Rust:

- ワークスペースの edition は Rust 2024。
- 公開 CLI ロジックは `rust/dotfiles-cli`、保守コマンドは `rust/xtask`、共通補助は `rust/dotfiles-core` に置く。
- 責務を混在させない。dispatcher ファイルに端末 I/O、信号方針、暗号補助、ワイヤ形式、テストを過密に載せない。
- `anyhow` はリポジトリの Result 別名 を通して使い、panic ではなく 文脈付きで伝播する。
- 単純な変換・抽出は 反復子と `collect` を優先する。
- `match collection.len()` ではなく スライスパターン、`is_empty`、ドメイン状態で分岐する。
- 不要な `mut` を導入しない。必要性を `git diff` で確認する。
- 閉じた集合を 生文字列 で渡さず 列挙型/新規型 で表現する。
- リポジトリ由来 Rust に `unsafe` を導入しない。
- テストを含め `unwrap` と `expect` を使わない。
- 警告を残さない。

Nix:

- 利用者設定は Home Manager、ホスト/システム設定は nix-darwin に置く。
- 明示的な 破壊的移行 依頼がない限り公開 flake API を維持する。
- 再利用モジュールに実ユーザー名、実ホスト名、マシン固有パスを埋め込まない。
- Nix 整形は `cargo xtask check static` が使う flake formatter に従う。

Shell/zsh:

- `scripts/bootstrap.sh` は導入クリティカルとして扱い、可搬に保ち `bash -n` で構文検証できる状態を維持する。
- zsh 挙動は `rust/tests/checks/src/zsh.rs` の前提（TAB、fzf-tab、autosuggestions、syntax highlighting、PATH 除外）と整合させる。
- アプリ管理の shell 注入や Docker 認証など、利用者ローカル可変状態はリポジトリ外に置く。

Lua/Neovim:

- 設定は `config/nvim/lua/omy/` 配下で既存の `configs` / `mappings` / `autocmds` 構造を維持する。
- 大規模な構造変更より局所変更を優先する。

## セキュリティ

- マシン秘密情報、認証情報、API トークン、Docker 認証状態、SSH 秘密鍵、アプリ セッションファイル をコミットしない。
- 意図的に宣言化する場合を除き、マシン固有の可変状態を Home Manager モジュールに入れない。
- Homebrew taps は flake inputs で固定されるため、設計変更がない限り可変 tap 運用を導入しない。

## コミット規約

- コミット関連作業は 副エージェント に委譲し、指示は「`AGENTS.md` を読みコミット作業を行う」だけにする。
- Conventional Commits 形式 `<type>(<scope>): <description>` を使う。
- `feat`、`fix`、`docs`、`refactor`、`test`、`chore`、`build` を基本種別とする。
- 説明文は日本語、type/scope は ASCII のままにする。
- 1 コミット 1 論理変更を原則とし、必要なら分割する。
- コミット境界判断では `git status` と `git diff` を必ず確認する。
- 検証の省略・不能が重要な場合はコミット本文で明示する。
- コミット 副エージェント は検証コマンドを実行しない。検証責務は親エージェントにある。

## プルリクエスト 規約

- PR 関連作業は 副エージェント に委譲し、指示は「`AGENTS.md` を読み PR 作業を行う」だけにする。
- `main` へ直接 push しない。変更は 機能ブランチ と PR 経由で取り込む。
- ブランチ名は `<type>/<scope>-<short-kebab-description>` とし、小文字 を維持する。
- PR には恒久的な変更内容、必要性、実施検証を記述し、チャット履歴や作業実況は書かない。
- 利用者可視の挙動変更、bootstrap 変更、module API 変更、生成物削除、移行手順 は明示する。
- 利用者可視コマンド、bootstrap 挙動、module 境界、zsh キー挙動 を変更した場合は `README.md` を更新する。
- 期待チェックを省略・未実施・dev shell 外実行した場合は PR に明記する。
