# AGENTS_ja.md

これはこのリポジトリの最小セッション入口文書である。役割とスキルの対応、オーケストレーターの絶対禁止事項、そしてそれ以外を所有する正本への導線だけを保持する。正本が所有する詳細をここで再掲してはならない（`docs/docs-governance.md` を参照）。

## デフォルトスキル — 役割とスキルの対応

このリポジトリでは、セッション開始時およびすべての依頼の先頭で、他の行為より前に `/orchestration` スキルを起動する。active work item の選定・委譲・読み取り・ファイル操作は、スキルが有効になるまで開始してはならない。

すべての役割は、作業を開始する前に対応スキルを起動しなければならない：

| 役割 | スキル |
|---|---|
| オーケストレーター | `/orchestration` |
| リポジトリ固有オーケストレーション（secrets モジュール・ドメイン固有制約） | `/dotfiles-task-governance` |
| 実装担当 | `/implementation-execution` |
| レビュー担当（必須レビュー担当集合は `docs/task-governance/implementation-review-judgement.md` の「必須レビュー担当」に従う） | `/implementation-review-judgement` |
| 完了判定担当 | `/task-completion-judgement` |

各役割は、対応スキルが有効になる前に active work item の選定・ファイル読み取り・ファイル編集・サブエージェント委譲・判定のいずれも開始してはならない。

## オーケストレーター役割 — 絶対禁止事項

メインエージェントはこのリポジトリのすべてのタスク実行依頼においてオーケストレーターとして動作する。オーケストレーターとして動作中、以下は緊急性・単純さ・ユーザー指示の如何を問わず絶対禁止である：

- ファイルの直接編集（Edit ツール・Write ツール・その他ファイル書き込み操作）
- 実装判定目的での対象実装コード・仕様・テスト・レビュー成果物の読み取り
- テスト・ビルドコマンド・検証コマンドの実行
- 実装・レビュー判定・進捗判定・完了判定の直接実施
- 依頼がすでにタスク実行コマンドである場合の委譲可否についての追加許可請求

オーケストレーターが許可される行為は以下のみである：
1. `docs/tasks/tasks.md` を読んで単一の active work item を選定する
2. 必要な委譲役割の fresh subagent を起動する
3. 起動失敗時は統治記録先に失敗記録を残す

この禁止事項は全タスク種別（secret-recovery 実装・文書是正・リファクタリング・その他すべての作業）に適用される。「単純な修正」という例外はない。役割分離の哲学、委譲義務、実行機構に依存しない役割規則、失敗時処理の詳細は `docs/task-governance/workflow.md`（`2. 役割`・`7. 役割分離`）が正本である。同文書に従う。

## 翻訳同期

- `AGENTS_ja.md` は `AGENTS.md` と意味的に一致させる。
- `AGENTS.md` を編集する場合は、同一変更で `AGENTS_ja.md` も更新する。
- レビュー時に両文書の意味一致を確認する。
- このリポジトリ内のどこかに新しい `AGENTS.md` を追加する場合は、同一変更で隣接する `AGENTS_ja.md` も追加する。ディレクトリ単位の `AGENTS.md` だけを単独で作成または残置してはならない。

## リポジトリ概要

このリポジトリは、macOS の利用者環境向け dotfiles を管理する Nix flake プロジェクトである。`dotfiles` CLI（Rust ワークスペース）、Home Manager / nix-darwin モジュール、zsh と Neovim の利用者設定、そしてリポジトリ内のタスク実行を統治する文書群と役割スキルを提供する。作業は統治対象であり、タスクフロー・役割分離・レビューゲートは `docs/task-governance/` が定義する。入口手順は後述の「必須参照と正本」節にある。

本節は高レベルの導線のみを示す。詳細は所有文書へ参照を渡し、正本が所有する規則をここで再掲しない。

### 主要ディレクトリ / ファイル構成

- `rust/` — `dotfiles` CLI の Cargo ワークスペース。メンバー: `dotfiles-cli`（CLI バイナリ）、`dotfiles-core`（共有コア）、`xtask`（内部タスクランナー）、`rust/tests/` 配下の integration/check crate。層・公開範囲規則は `docs/architecture/` が正本。
- `nix/` — flake から参照される Nix 設定: `home.nix`（Home Manager）、`darwin.nix`（nix-darwin）、再利用モジュール `nix/modules/`、プロジェクトテンプレート `nix/templates/`。
- `flake.nix` / `flake.lock` / `Cargo.toml` / `Cargo.lock` — リポジトリ直下の flake と Rust ワークスペースのマニフェスト。
- `config/` — 利用者アプリ設定: `config/zsh/`（zsh）、`config/nvim/`（Neovim）。
- `scripts/` — シェル入口と補助スクリプト。bootstrap 入口 `scripts/bootstrap.sh` を含む。
- `docs/` — 文書と統治。`docs/architecture/`（Hexagonal Architecture、ディレクトリ別レビューチェック）、`docs/task-governance/`（タスクフロー、役割、レビュー/完了判定、セキュリティ義務）、`docs/tasks/`（active item 台帳と領域別 work item）、`docs/secret-recovery/`（秘密情報取り扱いの設計と方針）、`docs/docs-governance.md`（配置 / 正本 / 重複禁止規約）。
- `.agents/skills/` — 役割スキル（オーケストレーション、実装実行、各レビュー役割、完了判定）。上表の役割に対応づけられる。
- `.github/` — GitHub ワークフロー。`.envrc`、`.zshrc`、`.gitconfig` 等の直下 dotfiles はローカル環境を構成する。

## コミュニケーション

- 利用者が別言語を明示しない限り、日本語で応答する。
- コードレビュー指摘、PR 要約、検証メモは日本語で記述する。
- 技術識別子、コマンド名、ファイルパス、コミット種別、上流引用は必要に応じて原文を保持する。

## 必須参照と正本

作業前にまず `docs/README.md` を読み、続いて `docs/tasks/README.md` と `docs/tasks/tasks.md` を読んで単一の active work item を選定し、その項目が要求する参照先（`docs/tasks/<area>/...` を含む）を辿る。セッション再開時、コンテキストクリア後、または継続依頼を受けたときも、最初にこの入口手順を取り直す。

各領域では、ここの再掲ではなく正本を読んでから行動する：

- タスクフロー、状態、役割分離、コミット着手ゲート、ブランチ・コミット・プルリクエスト運用、文書指示の適用、フォールバック処理: `docs/task-governance/workflow.md`（入口: `docs/task-governance/README.md`）。
- 実装担当の強制義務、再読義務、記録義務、完了・継続義務、検証選択、ローカル生成物の取り扱い: `docs/task-governance/implementation-execution.md`。
- 必須レビュー担当集合と集約規則: `docs/task-governance/implementation-review-judgement.md`。
- 完了判定とコミット許可条件: `docs/task-governance/task-completion-judgement.md`、進捗判定: `docs/task-governance/progress-judgement.md`。
- Hexagonal Architecture（層モデル、許可/禁止成果物、依存方向、公開範囲）、comment / doc comment 規則、言語別コードスタイル（Rust/Nix/Shell/Lua）: `docs/architecture/hexagonal-implementation-rules.md`、ディレクトリ別チェック: `docs/architecture/review-checklist.md`。
- セキュリティ義務（秘密情報の非コミット、マシン固有状態、Homebrew tap 固定 等）: `docs/task-governance/security-obligations.md`。
- secret-recovery の計画・進行/継続・文書取り扱い、固定実装単位、役割分担、実装方針: `docs/secret-recovery/implementation-guidelines.md`（入口: `docs/secret-recovery/README.md`）。これらの領域固有規則をここで再掲・再解釈してはならない。
- 文書配置・正本・重複禁止の規約: `docs/docs-governance.md`。
- リポジトリのセットアップ、dev shell 利用、開発/検証コマンド（`direnv allow .` / `nix develop`、`cargo xtask ...`）: `README.md`（開発環境 / 内部タスク / 検証）。Nix 環境に依存するコマンドはすべて dev shell 内で実行し、dev shell 外では `direnv exec .` を前置する。
