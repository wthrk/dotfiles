# repo-global ガバナンス文書整合タスク

この文書は repo-governance の進捗台帳である。進め方は [../../task-governance/workflow.md](../../task-governance/workflow.md#タスク運用ワークフロー) を参照する。

## 作業項目一覧

### ガバナンス文書整合

- 状態: `完了`
- 主成果物: `文書差分`
- 作業定義文書: [work-items/global-documentation-remediation.md](work-items/global-documentation-remediation.md#repo-global-ガバナンス文書整合)
- レビュー記録: [review-artifacts/global-documentation-remediation/review-2026-05-22.md](review-artifacts/global-documentation-remediation/review-2026-05-22.md)
- 現行サイクル確認/レビュー記録（2026-05-22）:
  - [review-artifacts/global-documentation-remediation/confirmation-2026-05-22.md](review-artifacts/global-documentation-remediation/confirmation-2026-05-22.md)
  - [review-artifacts/global-documentation-remediation/review-2026-05-22.md](review-artifacts/global-documentation-remediation/review-2026-05-22.md)
- 履歴確認/レビュー記録（2026-05-21）:
  - [review-artifacts/global-documentation-remediation/confirmation.md](review-artifacts/global-documentation-remediation/confirmation.md)
  - [review-artifacts/global-documentation-remediation/review.md](review-artifacts/global-documentation-remediation/review.md)
- 粗粒度進捗（履歴）: [issue-01-progress.md](issue-01-progress.md#issue-01-repo-global-文書整合-粗粒度進捗)
- 対象文書パス:
  - `.agents/skills/AGENTS.md`
  - `.agents/skills/AGENTS_ja.md`
  - `.agents/skills/dotfiles-task-governance/SKILL.md`
  - `.agents/skills/implementation-execution/SKILL.md`
  - `.agents/skills/implementation-review-judgement/SKILL.md`
  - `.agents/skills/task-completion-judgement/SKILL.md`
  - `.agents/skills/`
  - `AGENTS.md`
  - `AGENTS_ja.md`
  - `docs/docs-governance.md`
  - `docs/task-governance/README.md`
  - `docs/task-governance/implementation-execution.md`
  - `docs/task-governance/workflow.md`
  - `docs/task-governance/implementation-review-judgement.md`
  - `docs/task-governance/progress-judgement.md`
  - `docs/task-governance/task-completion-judgement.md`
  - `docs/task-governance/task-file-contract.md`
  - `docs/task-governance/review-artifacts/`
  - `docs/tasks/README.md`
  - `docs/tasks/tasks.md`
  - `docs/tasks/repo-governance/README.md`
  - `docs/tasks/repo-governance/`
  - `docs/secret-recovery/implementation-guidelines.md`（cross-area 追補対象）
  - `docs/tasks/secret-recovery/README.md`（移管支援）
  - `docs/tasks/secret-recovery/tasks.md`（移管支援）
  - `docs/tasks/secret-recovery/issue-11-progress.md`（移管支援）
  - `docs/tasks/secret-recovery/review-artifacts/README.md`（移管支援）
  - `docs/tasks/secret-recovery/work-items/README.md`（移管支援）
  - `docs/tasks/secret-recovery/work-items/final-documentation.md`（移管支援）
- 過去サイクル実行記録（履歴・次サイクル再利用禁止）:
  - 実装担当: `impl-agent-final-doc-closeout`
  - 実装担当 agent/run 識別子: `agent:impl-final-doc-closeout / run:2026-05-21-finaldoc-impl-002`
  - レビュー担当一覧:
    - 構造レビュー担当: `reviewer-structure-finaldoc-closeout`
    - 構造レビュー担当 agent/run 識別子: `agent:review-structure-finaldoc-closeout / run:2026-05-21-finaldoc-rs-002`
    - 運用整合レビュー担当: `reviewer-ops-finaldoc-closeout`
    - 運用整合レビュー担当 agent/run 識別子: `agent:review-ops-finaldoc-closeout / run:2026-05-21-finaldoc-ro-002`
    - セキュリティレビュー担当: `reviewer-security-finaldoc-closeout`
    - セキュリティレビュー担当 agent/run 識別子: `agent:review-security-finaldoc-closeout / run:2026-05-21-finaldoc-rsec-002`
    - 仕様適合レビュー担当: `reviewer-spec-finaldoc-closeout`
    - 仕様適合レビュー担当 agent/run 識別子: `agent:review-spec-finaldoc-closeout / run:2026-05-21-finaldoc-rsp-002`
    - 参照整合レビュー担当: `reviewer-reference-finaldoc-closeout`
    - 参照整合レビュー担当 agent/run 識別子: `agent:review-reference-finaldoc-closeout / run:2026-05-21-finaldoc-rr-002`
  - 進捗判定担当: `progress-judge-finaldoc-closeout`
  - 進捗判定担当 agent/run 識別子: `agent:progress-finaldoc-closeout / run:2026-05-21-finaldoc-pj-002`
  - 着手順序: `規約計画 -> 実装計画 -> 規約文書更新 -> 確認 -> レビュー -> 必要時の後続対応`
  - 役割別フォールバック記録（6項目必須）:
    - 対象役割: `セキュリティレビュー担当`
    - 起動失敗理由: `security reviewer runtime の認証失敗`
    - 起動失敗証跡: `2026-05-21T14:39+09:00 auth failed while launching security reviewer`
    - 代替実行者: `fallback-reviewer-security-finaldoc`
    - 代替実行者 agent/run 識別子: `agent:review-security-fallback-finaldoc / run:2026-05-21-finaldoc-rsec-fallback-001`
    - no-reuse 規則充足根拠: `代替実行者は構造/運用/仕様/参照整合レビュー担当とも別 agent`
  - 現行対象再読確認: `2026-05-21: 指定対象（docs/README.md, docs/task-governance/README.md, docs/tasks/README.md, repo-governance 台帳/証跡, AGENTS.md）を再読し、最終クローズ記録を更新`
  - 境界注記: `documentation-remediation として文書整合記録を更新。コード差分なし`
  - 境界注記（cross-area 対象と governing source の分離）: `docs/secret-recovery/implementation-guidelines.md は同一変更セットで整合対象に含める active cross-area documentation target だが、repo-governance の判断規則・進捗判定の governing source には含めない。参照順は root active ledger である docs/tasks/tasks.md を入口とし、その active work item が要求する docs/task-governance/* と docs/tasks/repo-governance/* を governing source として追う。included documentation targets には .agents/skills/*.md と repo-global 文書群を含め、実差分スコープと一致させる。`
  - 移管支援文書の扱い: `secret-recovery 配下の移管支援文書は移管経路・履歴・導線の整合を担う補助成果物として扱う。`
- 次サイクル着手計画スロット（未設定）:
  - 実装担当: `未設定`
  - 確認担当: `未設定`
  - レビュー担当一覧: `未設定`
  - 進捗判定担当: `未設定`
  - 着手順序: `未設定`
- 実装状態: `完了`
- 固定実装単位トラッカー:

| 実装単位 | 状態 | 成果物 | 参照 |
| --- | --- | --- | --- |
| 規約計画 | 完了 | `work-items/global-documentation-remediation.md` | [workflow.md#タスク運用ワークフロー](../../task-governance/workflow.md#タスク運用ワークフロー) |
| 実装計画 | 完了 | `docs/tasks/tasks.md`（root active ledger）, `docs/tasks/repo-governance/tasks.md`（area ledger） | [task-file-contract.md#タスクファイルに必須の項目（最小）](../../task-governance/task-file-contract.md#タスクファイルに必須の項目最小) |
| 規約文書更新 | 完了 | `docs/tasks/repo-governance/` 配下文書 | [implementation-execution.md#実装実行規則](../../task-governance/implementation-execution.md#実装実行規則) |
| 確認 | 完了（2026-05-22 現行サイクル証跡） | `docs/tasks/repo-governance/review-artifacts/global-documentation-remediation/confirmation-2026-05-22.md` | [progress-judgement.md#進捗判定規則](../../task-governance/progress-judgement.md#進捗判定規則) |
| レビュー | 完了（2026-05-22 現行サイクル証跡・集約合格） | `docs/tasks/repo-governance/review-artifacts/global-documentation-remediation/review-2026-05-22.md` | [implementation-review-judgement.md#実装レビュー判定](../../task-governance/implementation-review-judgement.md#実装レビュー判定) |
| 必要時の後続対応 | 完了（不要） | `docs/tasks/repo-governance/review-artifacts/global-documentation-remediation/review-2026-05-22.md` | [workflow.md#タスク運用ワークフロー](../../task-governance/workflow.md#タスク運用ワークフロー) |

- 現行サイクル注記: `2026-05-22 の現行差分識別子 working-tree-current-2026-05-22 について、確認記録とレビュー記録（運用整合/構造・履歴/参照整合の no blockers、および集約後レビュー判定: 合格）を充足し、現行サイクルの documentation-remediation を完了として閉じた。差分スコープは total 28 paths（tracked 25 + untracked 3。untracked: docs/tasks/tasks.md, docs/tasks/repo-governance/review-artifacts/global-documentation-remediation/confirmation-2026-05-22.md, docs/tasks/repo-governance/review-artifacts/global-documentation-remediation/review-2026-05-22.md）で管理し、2026-05-21 完了記録は docs-remediation-final-documentation-2026-05-21-001 の履歴事実として保持して現行差分の判定とは分離する。`
- 現行サイクル役割実行証跡（2026-05-22）:
  - 実装担当: `impl-agent-repo-governance-current-cycle`
  - 実装担当 agent/run 識別子: `agent:impl-repo-governance-current-cycle / run:2026-05-22-repo-gov-impl-001`
  - 確認担当: `confirmer-repo-governance-current-cycle`
  - 確認担当 agent/run 識別子: `agent:confirm-repo-governance-current-cycle / run:2026-05-22-repo-gov-confirm-001`
  - レビュー担当（現行構造整合差分レビュー）: `reviewer-repo-governance-current-cycle`
  - レビュー担当 agent/run 識別子: `agent:review-repo-governance-current-cycle / run:2026-05-22-repo-gov-review-001`
  - 進捗判定担当: `progress-judge-repo-governance-current-cycle`
  - 進捗判定担当 agent/run 識別子: `agent:progress-repo-governance-current-cycle / run:2026-05-22-repo-gov-progress-001`
- 現行サイクル対象（working-tree-current-2026-05-22）:
  - `.agents/skills/AGENTS.md`
  - `.agents/skills/AGENTS_ja.md`
  - `.agents/skills/dotfiles-task-governance/SKILL.md`
  - `.agents/skills/implementation-execution/SKILL.md`
  - `.agents/skills/implementation-review-judgement/SKILL.md`
  - `.agents/skills/task-completion-judgement/SKILL.md`
  - `AGENTS.md`
  - `AGENTS_ja.md`
  - `docs/docs-governance.md`
  - `docs/secret-recovery/implementation-guidelines.md`
  - `docs/task-governance/implementation-execution.md`
  - `docs/task-governance/implementation-review-judgement.md`
  - `docs/task-governance/progress-judgement.md`
  - `docs/task-governance/task-completion-judgement.md`
  - `docs/task-governance/task-file-contract.md`
  - `docs/task-governance/workflow.md`
  - `docs/tasks/README.md`
  - `docs/tasks/repo-governance/README.md`
  - `docs/tasks/repo-governance/review-artifacts/README.md`
  - `docs/tasks/repo-governance/issue-01-progress.md`
  - `docs/tasks/repo-governance/review-artifacts/global-documentation-remediation/confirmation-2026-05-22.md`
  - `docs/tasks/repo-governance/review-artifacts/global-documentation-remediation/review-2026-05-22.md`
  - `docs/tasks/repo-governance/tasks.md`
  - `docs/tasks/repo-governance/work-items/global-documentation-remediation.md`
  - `docs/tasks/secret-recovery/README.md`
  - `docs/tasks/secret-recovery/tasks.md`
  - `docs/tasks/secret-recovery/review-artifacts/_review-template.md`
  - `docs/tasks/tasks.md`

## 進捗記録

- `2026-05-21`: [docs/tasks/secret-recovery/issue-11-progress.md#sub-issue-一覧](../secret-recovery/issue-11-progress.md#sub-issue-一覧) の `#18 最終ドキュメント整理` 記録を引き継ぎ、台帳・作業定義・証跡の正本を repo-governance へ移管。
- `2026-05-21`: `対象文書パス` から削除済みの `docs/tasks/secret-recovery/review-artifacts/final-documentation/confirmation.md` と `docs/tasks/secret-recovery/review-artifacts/final-documentation/review.md` を除外し、削除履歴として本記録に保持。
