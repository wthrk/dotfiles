---
name: implementation-review-judgement
description: サブエージェントが実装レビュー開始準備判定と複数レビュアー結果の集約を行うときに使うスキル。
---

> **開始手順**: 何か実行する前にこのファイル全体を最後まで読むこと。このファイルが統括ソースであり、全節を読むまで作業を開始してはならない。

# 実装レビュー判定（集約役割）

このスキルは**集約役割**である。責務は、各レビュアースキルから返された判定を受け取り、集約判定を返すことだけ。個別レビュアー判定は、対応するレビュアースキルを使う独立 subagent が実施しなければならない。集約役割が個別判定を代行してはならない。

## レビュアースキル一覧

委譲先レビュアー役割とスキルファイルパスは次のとおり。

- **構造レビュー担当**: `.agents/skills/structural-review/SKILL.md`
- **仕様適合レビュー担当**: `.agents/skills/specification-conformance-review/SKILL.md`
- **セキュリティレビュー担当**: `.agents/skills/security-review/SKILL.md`
- **運用整合レビュー担当**: `.agents/skills/operational-consistency-review/SKILL.md`
- **テストレビュー担当**: `.agents/skills/test-review/SKILL.md`
- **ドキュメントレビュー担当**: `.agents/skills/documentation-review/SKILL.md`
- **アーキテクチャ整合レビュー担当**: `.agents/skills/architectural-consistency-review/SKILL.md`
- **参照整合レビュー担当**: `.agents/skills/reference-integrity-review/SKILL.md`

## 統括ソース

- `docs/task-governance/workflow.md` は役割割当、フォールバック、subagent 割当を統括する。
- `docs/task-governance/implementation-review-judgement.md` はレビュー開始ゲート、レビュアー役割、変更種別ごとの必須担当、判定形式、集約規則を統括する。この文書が正本であり、規則をここへ重複定義してはならない。

## 必須参照順

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/implementation-review-judgement.md`
5. `docs/tasks/README.md`
6. `docs/tasks/tasks.md`
7. active work item が要求する領域固有成果物（`docs/tasks/<area>/...`）
8. 関連する `docs/tasks/<area>/review-artifacts/...`

`docs/tasks/<area>/tasks.md` は active work item が明示参照している場合のみ必須。

## 使う場面

このスキルはレビュー開始ゲート確認とレビュー集約確認に使う。

アクター拘束: このスキルが有効な間、現在の実行者は上記統括ソースに拘束された implementation-review-judgement の集約役割である。

## 規則

- 集約役割の唯一責務は、必須レビュアー各員から判定を受領し、集約判定を返すこと。個別レビュアーの職務を自分で実行してはならない。
- `docs/task-governance/implementation-review-judgement.md` の `必須レビュー担当` 節を読み、変更種別ごとの必須レビュアー役割を決定すること。各必須レビュアーは separate な fresh subagent として起動し、対応するスキルファイルパスを個別指定しなければならない。
- 判定対象はレビュー開始条件と集約条件に限定すること。完了判定は `task-completion-judgement` へ委譲すること。
- 集約判定の形式、ラベル集合、適用規則は `docs/task-governance/implementation-review-judgement.md` を正本として適用すること。このファイルへ重複定義してはならない。
- 必須レビュアーの判定が返ったら、集約処理へ進む前にその判定を記録すること。
- 必須レビュアーを起動できない場合は、`docs/task-governance/implementation-review-judgement.md` が定める形式で、その事実と取り扱いを記録すること。
