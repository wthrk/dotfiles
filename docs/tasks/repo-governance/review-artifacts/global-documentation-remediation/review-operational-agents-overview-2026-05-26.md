# 運用整合レビュー — AGENTS overview + role-rule fix（2026-05-26）

判定: 合格

判定要約: 所見なし。前回 要修正（transport-agnostic role rule の dangling pointer）は正本 `docs/task-governance/workflow.md` `## 2. 役割` への復元で解消済みであり、AGENTS.md のポインタは解決する。AGENTS.md の baseline orientation（Repository Overview + Major Directory / File Layout）は high-level かつポインタ基準で復元され、正本複製を再導入していない。AGENTS.md から再配置された全規則は各正本に現存し、欠落はない。役割束縛表とオーケストレーター絶対禁止の中核は現存し正しい。

根拠:

- 差分の実在確認: `git diff HEAD --stat` で対象変更セットが実在することを確認した。governance 文書変更は AGENTS.md / AGENTS_ja.md（各 283 行変動）、docs/task-governance/workflow.md（+35）、docs/architecture/hexagonal-implementation-rules.md（+49）、docs/task-governance/implementation-execution.md（+24）、docs/task-governance/security-obligations.md、docs/secret-recovery/implementation-guidelines.md、docs/task-governance/README.md、.agents/skills 配下 3 ファイル。スコープ外の `rust/` 変更は本判定の対象外として扱い、参照していない。各ファイルの実本文と `git diff HEAD` を直接精査した。過去のレビュー記録・確認記録は判定根拠に用いていない（レビュー独立性）。

- (a) transport-agnostic role rule の正本復元と pointer 解決を確認:
  - `docs/task-governance/workflow.md` `## 2. 役割`（24 行目）に当該規則が現存する。内容は「エージェントの役割は受け取った委譲内容で決まり実行機構（実行トランスポート）では決まらない／スコープ限定の実装タスクを委譲されたエージェントは実装担当として直接実行し再委譲しない／外部 exec モード等の実行トランスポート経由であること自体はオーケストレーター制約の免除理由にならない／オーケストレーションを要する指示を受け取ったエージェントは実行機構の如何を問わずオーケストレーションを行わなければならない」を網羅する。
  - AGENTS.md（36 行目）/ AGENTS_ja.md（36 行目）の `Orchestrator Role — Absolute Prohibitions` 末尾は「The detailed role-separation philosophy, delegation obligations, transport-agnostic role rule, and failure-handling rules are owned by `docs/task-governance/workflow.md` (`2. 役割`, `7. 役割分離`)」と記し、正本セクション名が実在する見出し（`## 2. 役割` / `## 7. 役割分離`）に一致する。dangling pointer は残っていない。前回 要修正は実質的に解消済みと確認した。

- (b) baseline orientation が十分かつ正本複製を再導入していないことを確認:
  - AGENTS.md `## Repository Overview`（45-49 行）+ `### Major Directory / File Layout`（51-60 行）に、rust/ ワークスペース（メンバー列挙）、nix/、flake/Cargo マニフェスト、config/、scripts/、docs/（architecture / task-governance / tasks / secret-recovery / docs-governance.md）、.agents/skills/、.github/ の実構造を pointer 基準で記載。初回エージェントの方位付けに十分。
  - 49 行に「This section is high-level orientation only. It points to owning documents for detail and does not restate rules that a canonical source owns.」と明記され、層・公開範囲規則は `docs/architecture/` が正本、タスクフロー/役割分離/レビューゲートは `docs/task-governance/` が定義、と委譲している。具体規則本文の再掲はなく、`docs/docs-governance.md` の「README は導線のみ／正本を移す場合は二重正本を残さない」原則に適合する。orientation は entry doc 固有の責務であり canonical duplication ではない。

- (c) 再配置された全規則が各正本に現存し欠落がないことを確認（各正本の本文を直接照合）:
  - Code Style（Rust/Nix/Shell/Lua）→ `docs/architecture/hexagonal-implementation-rules.md` `## 言語別コードスタイル`（139 行）に現存し「リポジトリ共通の言語別コードスタイル規約の正本」と明示。
  - comment / doc comment 規則 → 同ファイル `## ドキュメントコメント規則`（122 行）。挙動変更時の近傍コメント更新規則も追加されている。
  - 実装担当の完了・継続義務 / 検証選択 / ローカル生成物の取り扱い → `docs/task-governance/implementation-execution.md` の `## 完了・継続義務`（53 行）/ `## 検証選択`（61 行）/ `## ローカル生成物の取り扱い`（72 行）に現存。`既存コード再読義務`（20 行）も AGENTS.md の "reread duties" pointer に一致。
  - セキュリティ義務（API トークン・Docker 認証状態・SSH 秘密鍵・アプリセッションファイルの非コミット、Home Manager マシン固有状態、Homebrew tap 固定）→ `docs/task-governance/security-obligations.md` `## 基本義務` に現存。
  - ブランチ・コミット・プルリクエスト運用 → `docs/task-governance/workflow.md` `## 8. ブランチ・コミット・プルリクエスト運用`（コミット着手条件は本書「6. コミット着手ゲート」を正本とする旨を含む）に現存。`docs/task-governance/README.md` の workflow.md 案内も「ブランチ・コミット・プルリクエスト運用」を追記して整合。
  - secret-recovery の進捗取り扱い（文書整合/実装の分離、`コード差分なし` の hard-stop、前提証跡同時更新を条件とする前進遷移）→ `docs/secret-recovery/implementation-guidelines.md` `### 確認`（98-100 行）に現存。
  - セットアップ / dev shell / 開発・検証コマンド → `README.md`（開発環境 / 内部タスク / 検証）に現存。dev shell 外で `direnv exec .` を前置する規則は `implementation-execution.md` `## 検証選択` が正本として保持。
  - リポジトリ全域で grep し、削除済み AGENTS.md セクション（Critical Planning Gate / Project Overview / Applying Document Instructions / Development Commands / ## Codex / `AGENTS.md#` アンカー）への dangling 参照が他文書・スキルに残っていないことを確認した（該当なし）。

- (d) 役割束縛と絶対禁止の中核が現存・正確であることを確認:
  - AGENTS.md `## Default Skills — Role-to-Skill Binding` の役割→スキル表（Orchestrator / dotfiles-task-governance / implementation-execution / implementation-review-judgement / task-completion-judgement）と「スキル起動前に作業を開始してはならない」制約は保持。
  - `## Orchestrator Role — Absolute Prohibitions`（21-36 行）の絶対禁止 5 項目と許可 3 行為（active item 選定の `docs/tasks/tasks.md` 読み取り／必要役割の fresh subagent 起動／起動失敗時の最小記録）は保持され、`docs/task-governance/workflow.md` の `## 2. 役割` / `## 7. 役割分離` と齟齬しない。
  - AGENTS_ja.md は AGENTS.md と節単位で意味的に一致し（Repository Overview / Major Directory・File Layout / Required References and Canonical Sources 全節対応）、翻訳同期規則を満たす。

- (e) その他の強制可能性 / 監査可能性の懸念:
  - AGENTS.md `## Required References and Canonical Sources`（68-83 行）の全ポインタ先（workflow.md・implementation-execution.md・implementation-review-judgement.md・task-completion-judgement.md・progress-judgement.md・hexagonal-implementation-rules.md・review-checklist.md・security-obligations.md・implementation-guidelines.md・docs-governance.md・README.md）の実在を確認した。各ポインタが指す見出し（`2. 役割`・`7. 役割分離`・各 implementation-execution.md 節・`言語別コードスタイル`・README.md の 開発環境/内部タスク/検証）も実在する。pointer 切れ・正本不在は認められない。
  - 文書是正のため、無関係な台帳・粗粒度進捗の同時同期を gate にしておらず、`docs/docs-governance.md` 及び workflow.md `6. コミット着手ゲート` の文書是正規定と整合する。強制可能性・監査可能性に具体的懸念は残らない。
