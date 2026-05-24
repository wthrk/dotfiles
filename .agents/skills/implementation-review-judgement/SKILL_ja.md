---
name: implementation-review-judgement
description: サブエージェントが実装レビュー開始準備の判定および複数レビュアー集約を行わなければならないときに使用するスキル。
---

# 実装レビュー判定

## 統括文書

- `docs/task-governance/workflow.md`: 役割割当、フォールバック、サブエージェント割当を統括する。
- `docs/task-governance/implementation-review-judgement.md`: レビュー開始ゲート、レビュアー役割、集約規則を統括する。
- `docs/task-governance/security-obligations.md`: レビュー判定および記録成果物に拘束力を持つセキュリティ制約を統括する。
- `docs/architecture/hexagonal-implementation-rules.md`: 構造レビュー時に適用しなければならないレイヤーベースのアーキテクチャ制約を統括する。

## 必須参照順

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/implementation-review-judgement.md`
5. `docs/task-governance/security-obligations.md`
6. `docs/architecture/hexagonal-implementation-rules.md`
7. `docs/tasks/README.md`
8. `docs/tasks/tasks.md`
9. アクティブ作業項目が要求する領域固有成果物（`docs/tasks/<area>/...`）
10. 関連する `docs/tasks/<area>/review-artifacts/...`

`docs/tasks/<area>/tasks.md` はアクティブ作業項目が明示的に参照している場合にのみ必須とする。

## 起動タイミング

このスキルはレビュー開始ゲートチェックおよびレビュー集約チェックに使用する。

アクター拘束: 本スキルが有効な間、現在の実行者は上記統括文書に拘束された実装レビュー判定役割のみとなる。

## 規則

- レビュー開始条件と集約条件のみを判定すること。
- 完了判定は `task-completion-judgement` へ委任すること。
- `docs/tasks/tasks.md` からアクティブ項目を選定し、レビュー対象がその項目と整合していることを検証し、その項目が参照するレビュー成果物および領域タスク定義を実行統括ソースとして従うこと。
- レビュアーの判定は以下の単一明示ラベルセットのみを使用して返すこと: `合格`、`要修正`、`不合格`。
- すべてのレビュアー応答は以下の正確な構造で開始すること:
  - `判定: <合格|要修正|不合格>`
  - `判定要約: <所見なし|主要論点要約>`
  - `根拠:`
- `通しません`、`No findings`、`指摘なし`、`no blockers`、`pass` などの自由形式の先頭判定を `判定` 行の代わりに使用してはならない。
- 判定が `合格` の場合は `判定要約: 所見なし` を使用すること。
- 懸念事項、残留リスク、未解決の疑問、フォローアップ項目、または運用上の依存関係が残っている場合、判定は少なくとも `要修正` でなければならない。是正条件を `根拠:` に記録し、`合格` を出力してはならない。
- 構造レビューは `docs/architecture/hexagonal-implementation-rules.md` のレイヤーベース規則を適用しなければならず、ファイル名固有の違反対象のみに依拠してはならない。`adapters/` ファイルが `pub`、`pub(crate)`、`pub(super)` のいずれかで、ポートトレイト実装でない項目を公開している場合は `判定: 不合格` を出力すること。ヘルパー関数（stdin読み取り関数、プロンプト関数、JSONデコーダー、ターミナルI/O関数）はどこで定義されていても、ポートトレイト実装ではない。
- secret復元コードのdiffをレビューする場合: 変更された各ファイルについて、`docs/architecture/hexagonal-implementation-rules.md` のsecretsモジュールレイヤーマッピングからそのレイヤーを特定し、そのファイルの内容がそのレイヤーの責任制約および禁止規則を満たしていることを検証すること。
- コンパイルは通るがレイヤーベース制約に違反するdiffは、テスト結果に関わらず `判定: 不合格` を受けなければならない。
