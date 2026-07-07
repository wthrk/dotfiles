# AGENTS_ja.md

これはこのリポジトリの最小セッション入口文書である。言語方針、スキル先行の入口順序、役割とスキルの対応、オーケストレーター禁止事項への導線、翻訳同期だけを保持する。

## コミュニケーション

- 利用者が別言語を明示しない限り、日本語で応答する。
- コードレビュー指摘、PR 要約、検証メモは日本語で記述する。
- 技術識別子、コマンド、パス、コミット種別、上流引用は必要に応じて原文を保持する。

## スキル先行入口

- すべての top-level task-execution request では、タスク参照を読む前、または役割作業を始める前に `/orchestration` を起動する。
- 下記の最初に読む参照は、skill 起動前作業ではなく、`/orchestration` skill の Required Reading Order として読む。

## 最初に読む参照

- [docs/README.md](docs/README.md) から開始し、続いて [docs/task-governance/README.md](docs/task-governance/README.md) と [docs/task-governance/workflow.md](docs/task-governance/workflow.md) を読む。
- 文書配置、正本扱い、重複禁止は [docs/docs-governance.md](docs/docs-governance.md) を適用する。
- 役割詳細は `.agents/skills/*/SKILL.md` が所有する。ここで再掲しない。

## 役割とスキルの対応

すべての役割は、役割作業の前に指定スキルを起動しなければならない。

| 役割 | スキル |
|---|---|
| オーケストレーター | `/orchestration` |
| リポジトリ固有統治補助 | `/dotfiles-task-governance` |
| 実装担当 | `/implementation-execution` |
| レビュー集約 | `/implementation-review-judgement` |
| 完了判定 | `/task-completion-judgement` |

個別レビュー担当は次のスキルファイルを使う。

| レビュー担当 | スキルファイル |
|---|---|
| 構造レビュー担当 | `.agents/skills/structural-review/SKILL.md` |
| 運用整合レビュー担当 | `.agents/skills/operational-consistency-review/SKILL.md` |
| セキュリティレビュー担当 | `.agents/skills/security-review/SKILL.md` |
| 仕様適合レビュー担当 | `.agents/skills/specification-conformance-review/SKILL.md` |
| 外部依存適合レビュー担当 | `.agents/skills/external-dependency-conformance-review/SKILL.md` |
| テストレビュー担当 | `.agents/skills/test-review/SKILL.md` |
| ドキュメントレビュー担当 | `.agents/skills/documentation-review/SKILL.md` |
| アーキテクチャ整合レビュー担当 | `.agents/skills/architectural-consistency-review/SKILL.md` |
| 参照整合レビュー担当 | `.agents/skills/reference-integrity-review/SKILL.md` |

委譲された役割エージェントは、その delegated task についてメインエージェントにはならない。委譲された実装担当は `/implementation-execution` から開始し、同じ delegated task について `/orchestration` を起動せず、その委譲済み実装割当に対して追加のサブエージェントを起動しない。

## オーケストレーター禁止事項

オーケストレーターの絶対禁止事項と許可行為は [docs/task-governance/workflow.md](docs/task-governance/workflow.md) が定義する。このファイルでは重複定義しない。

## 翻訳同期

- `AGENTS_ja.md` は `AGENTS.md` と意味的に一致させる。
- `AGENTS.md` を編集する場合は、同じ変更で `AGENTS_ja.md` も更新する。
- このリポジトリ内のどこかに新しい `AGENTS.md` を追加する場合は、同じ変更で隣接する `AGENTS_ja.md` も追加する。
