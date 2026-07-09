# docs 文書運用規約

この文書は `docs/` 配下の配置、正本、重複禁止を定義する。

## 配置規則

- `docs/README.md`: 恒久文書全体の入口。
- `docs/task-governance/README.md`: 共通タスク運用規約の入口。
- `docs/architecture/README.md`: アーキテクチャ規約の入口。
- `docs/<area>/README.md`: 領域仕様・設計文書の入口。

作業台帳、完了済み work item、確認記録、レビュー記録、issue progress、current-cycle 記録は恒久文書ではない。必要な履歴は GitHub issue / PR / git history で追跡し、repo 内 docs に再掲しない。

## 正本規則

- 共通運用規約は `docs/task-governance/` に置く。
- アーキテクチャ規約は `docs/architecture/` に置く。
- secret-recovery の仕様・設計・runbook・secret handling は `docs/secret-recovery/` に置く。
- 役割スキルの初動・担当境界は `.agents/skills/*/SKILL.md` に置く。
- ユーザー指定の GitHub issue、PR、または明示タスクが作業単位であり、repo 内 task ledger を正本にしない。

## 記載規則

- README は導線だけを記載し、本文規約を再掲しない。
- 仕様・設計・運用規約・作業履歴の責務を混在させない。
- 正本が所有する規則を別文書へ長文再掲しない。必要な場合は正本への参照にする。
- 後付けの文書書換えだけで充足できる形式要件を gate にしてはならない。
- exact file-set、file-count、actor/run bookkeeping、current-cycle 文言、補助記録の完全同期は review gate / commit gate / PR 対応義務の代替にしない。
- PR review comment への採用/不採用返信と resolved 状態の維持は、補助記録同期とは別の実作業義務として扱う。
- 文書ファイル（.md を含む）の一括置換に Python スクリプト・sed スクリプトを使ってはならない。文書の変更は常に edit ツールで行う。

## 参照規則

- 参照は存在する恒久文書、コードパス、または外部の GitHub issue / PR へ向ける。
- 削除済みの作業管理文書へリンクしない。
- 正本を移す場合は旧記述を削除または参照化し、二重正本を残さない。
