# 参照整合レビュー（AGENTS overview + role-rule fix, 2026-05-26）

判定: 合格

判定要約: 所見なし

根拠:

- **差分の実在確認**: `git status` / `git diff HEAD` を実行し、本レビュー対象の文書是正差分が実在することを独立に確認した。変更ファイルは `AGENTS.md`、`AGENTS_ja.md`、`docs/task-governance/workflow.md`、`docs/task-governance/README.md`、`docs/task-governance/implementation-execution.md`、`docs/task-governance/security-obligations.md`、`docs/secret-recovery/implementation-guidelines.md`、`docs/architecture/hexagonal-implementation-rules.md`、`.agents/skills/dotfiles-task-governance/SKILL.md`、`.agents/skills/orchestration/SKILL.md`、`.agents/skills/orchestration/SKILL_ja.md`。`rust/` 配下の無関係差分はスコープ外として除外した。過去の確認記録に依存せず、各ファイルを直接読んで判定した。

- **(a) transport-agnostic role rule ポインタの解決**: AGENTS.md L36 / AGENTS_ja.md L36 は「実行機構に依存しない役割規則」を `docs/task-governance/workflow.md`（`2. 役割`・`7. 役割分離`）所有として参照する。`git diff` で当該ルール（「エージェントの役割は、受け取った委譲内容によって決まり、実行機構（実行トランスポート）によっては決まらない…」）が `## 2. 役割` 直下（現行 workflow.md L24）に実際に復元されていることを確認した。`## 7. 役割分離` も実在する（L95）。ポインタは dangle せず解決する。

- **(b) Repository Overview / Major Directory File Layout の全パス実在確認**: 概要節が名指しする全パス/ディレクトリを `test -e` で個別検証し、すべて実在することを確認した。`rust/` とそのメンバー（`dotfiles-cli`・`dotfiles-core`・`xtask`・`dotfiles-cli-secrets-test-contract`・`dotfiles-cli-secrets-test-stub`・`rust/tests/`）、`nix/`（`home.nix`・`darwin.nix`・`nix/modules/`・`nix/templates/`）、root manifests（`flake.nix`・`flake.lock`・`Cargo.toml`・`Cargo.lock`）、`config/`（`config/zsh/`・`config/nvim/`）、`scripts/`（`scripts/bootstrap.sh`）、`docs/` 各サブディレクトリ（`docs/architecture/`・`docs/task-governance/`・`docs/tasks/`・`docs/secret-recovery/`・`docs/docs-governance.md`）、`.agents/skills/`、`.github/`、root dotfiles（`.envrc`・`.zshrc`・`.gitconfig`）。存在しないパスを名指しする記述は0件。

- **(c) AGENTS.md / AGENTS_ja.md の意味的同期**: 両ファイルを直接通読し、見出し構成が完全に対応することを確認（各8見出し、同順、Translation Synchronization / 翻訳同期を含む）。`### Major Directory / File Layout` と `### 主要ディレクトリ / ファイル構成` は箇条書き8項目で一致、`## Required References and Canonical Sources` と `## 必須参照と正本` は9項目で一致。概要文・layout 各項・ポインタの対応も一致する。構造ドリフトなし。

- **(d) その他の追加ポインタの解決**: 変更セット内で追加/変更された参照を直接検証した。AGENTS.md L78（`docs/architecture/hexagonal-implementation-rules.md` の層モデル・comment/doc-comment 規則・言語別コードスタイル、`docs/architecture/review-checklist.md` のディレクトリ別チェック）に対し、hexagonal-implementation-rules.md に `## 層モデル`(L39)・`## ドキュメントコメント規則`(L122)・`## 言語別コードスタイル`(L139) が実在し、`review-checklist.md` も実在することを確認。AGENTS.md L82 の `README.md`（開発環境 / 内部タスク / 検証）に対し README.md に `## 開発環境`・`## 内部タスク`・`## 検証` が実在。implementation-execution.md 追加節が参照する `rust/tests/checks/src/static_checks.rs`・`rust/tests/checks/src/zsh.rs` も実在。docs/task-governance/README.md L7 の `workflow.md#タスク運用ワークフロー` アンカー、workflow.md 内の「6. コミット着手ゲート」(L83) も解決。役割テーブルの各スキルパス（`.agents/skills/<role>/SKILL.md`）も全件実在。`.claude/skills -> ../.agents/skills` シンボリックリンクも解決を確認。

- **(e) 除去内容に対する dangling 参照の不在**: `docs/`・`.agents/`・`README.md` を `Code Style`・`Architecture Constraints`・`Critical Planning Gate`・`Development Commands`・`Commit Rules` および `AGENTS.md#…` アンカーで掃引した結果、削除された旧 AGENTS セクションを指す live 参照は0件。一致したのは `review-artifacts/` 配下の append-only 監査記録（過去状態を記述するもので live な相互参照ではない）のみ。とくに hexagonal-implementation-rules.md は旧来 `[AGENTS.md](../../AGENTS.md) の Code Style コメント規則を継承し` という AGENTS 依存記述を削除し「この comment / doc comment 規則はリポジトリ共通のコメント規約の正本である」へ置換しており、正本移管後に AGENTS への dangling が残らないよう整合化されている。

- **(f) 正本複製禁止規則（docs/docs-governance.md）への適合**: Repository Overview は冒頭で「high-level orientation only / 高レベルの導線のみ」と宣言し、各 layout 項目が詳細を所有文書へ委譲する（例: 「Layer/visibility rules are owned by `docs/architecture/`」「層・公開範囲規則は `docs/architecture/` が正本」）。正本が所有する規則本文の再掲は確認されず、orientation の範囲に留まる。docs-governance.md の「README は導線のみ・本文規約を再掲しない」「正本を移す場合は旧記述を削除または参照化し二重正本を残さない」に適合する。

- **スコープ確認**: `rust/` の無関係差分は本レビューでは一切扱っていない。台帳・rust/ ファイルの編集、コミットは行っていない。
