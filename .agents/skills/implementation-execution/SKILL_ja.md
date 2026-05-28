---
name: implementation-execution
description: サブエージェントが実装作業を割り当てられ、リポジトリの実行義務に従ってdiffを生成しなければならないときに使用するスキル。
---

# 実装実行

## 統括文書

- `docs/task-governance/workflow.md`: 役割割当、フォールバック、サブエージェント割当を統括する。
- `docs/task-governance/implementation-execution.md`: 実行義務、エビデンス記録、是正スコープを統括する。
- `docs/architecture/hexagonal-implementation-rules.md`: レイヤーベースの責任境界、各レイヤーで許可・禁止される成果物、可視性規則を統括する。

## 必須参照順

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/implementation-execution.md`
5. `docs/architecture/hexagonal-implementation-rules.md`
6. `docs/tasks/README.md`
7. `docs/tasks/tasks.md`
8. `docs/tasks/<area>/README.md`
9. `docs/tasks/<area>/work-items/<item>.md`
10. アクティブ作業項目が要求する領域固有成果物（`docs/tasks/<area>/...`）
11. `docs/<area>/implementation-guidelines.md`（存在する場合）

`docs/tasks/<area>/tasks.md` はアクティブ作業項目が明示的に参照している場合にのみ必須とする。

## 起動タイミング

このスキルは実装割当に使用する。

アクター拘束: 本スキルが有効な間、現在の実行者は上記統括文書に拘束された実装実行者役割のみとなる。

## 規則

- `docs/task-governance/implementation-execution.md` の必須参照、ファイルカテゴリ再読義務、記録要件に従うこと。
- `docs/tasks/tasks.md` からアクティブ項目を選定し、その項目の `docs/tasks/<area>/...` 配下の必須参照を実行統括ソースとして従うこと。
- レビューフィードバックの是正スコープについては、`docs/task-governance/implementation-execution.md` の拘束力あるフルスコープレビュアー視点規則に従うこと。
- 実装規則のテキストをそのまま適用すること（`最小構成で済まそうとしてはならない。` を含む）。
- 最小diffおよび継承構造の保持は目標ではない。準拠構造への再設計を行うこと（必要に応じてモジュール/ドキュメント境界のゼロベース書き直しを含む）。
- `adapters/` にコードを書く前に、実装がポートトレイト実装のみを公開していることを検証すること。`pub`、`pub(crate)`、`pub(super)` のいずれで宣言されていても、ポートトレイト実装でない項目はレイヤー違反であり、削除するかプライベートにしなければならない。stdin読み取り関数、プロンプト関数、JSONデコーダー、ターミナルI/O関数などのヘルパー関数はポートトレイト実装ではないため、以前 `pub(crate)` であったとしても、アダプターファイル内で `fn`（プライベート）でなければならない。
- `application/` にコードを書く前に、アダプターの具体型がインポートされておらず、`println!` / stdin読み取りが存在しないことを検証すること。
- `docs/architecture/hexagonal-implementation-rules.md` のレイヤーベース規則はファイル名固有規則より優先される。ファイル名固有の違反対象（例: `yubikey.md` の V1〜V16）が解消されているように見えても、レイヤーベースの違反が残っている場合、その項目は解消されていない。
