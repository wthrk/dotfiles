---
name: implementation-execution
description: サブエージェントが実装作業を割り当てられ、リポジトリの実行義務に従ってdiffを生成しなければならないときに使用するスキル。
---

# 実装実行

## 統括文書

- `docs/task-governance/workflow.md`: 役割割当、フォールバック、サブエージェント割当を統括する。
- `docs/task-governance/implementation-execution.md`: 実行義務、エビデンス記録、是正スコープを統括する。
- `docs/task-governance/security-obligations.md`: 実装と証跡記録に拘束力を持つセキュリティ制約を統括する。
- `docs/architecture/hexagonal-implementation-rules.md`: レイヤーベースの責任境界、各レイヤーで許可・禁止される成果物、可視性規則を統括する。

## 必須参照順

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/workflow.md`
4. `docs/task-governance/implementation-execution.md`
5. `docs/task-governance/security-obligations.md`
6. `docs/architecture/hexagonal-implementation-rules.md`
7. `docs/architecture/review-checklist.md`（ディレクトリ別チェック項目 — レビュー時だけでなく実装時にも適用する）
8. `docs/tasks/README.md`
9. `docs/tasks/tasks.md`
10. `docs/tasks/<area>/README.md`
11. `docs/tasks/<area>/work-items/<item>.md`
12. アクティブ作業項目が要求する領域固有成果物（`docs/tasks/<area>/...`）
13. `docs/<area>/implementation-guidelines.md`（存在する場合）

`docs/tasks/<area>/tasks.md` はアクティブ作業項目が明示的に参照している場合にのみ必須とする。

## 起動タイミング

このスキルは実装割当に使用する。

アクター拘束: 本スキルが有効な間、現在の実行者は上記統括文書に拘束された実装実行者役割のみとなる。
親オーケストレーターから委譲されたタスクでこのスキルを渡された場合、委譲実装担当が最初に読むスキルは本スキルである。そのタスクのオーケストレーションは完了済みとして扱う。同じ delegated task について `/orchestration` または `$dotfiles-task-governance` を読んだり起動したりせず、active item を再選定せず、追加の subworker を起動しない。

## 規則

- メインエージェントがオーケストレーターである。委譲された実装担当は、その delegated task におけるメインエージェントではなく、main-agent 向けのオーケストレーション入口規則を自分へ適用してはならない。
- `AGENTS.md` および `docs/task-governance/workflow.md` のオーケストレーター専用指示は、現在の依頼自体が top-level agent として自分へ宛てられた新しいタスク実行依頼でない限り、親オーケストレーターへの制約として読むこと。
- 委譲された実装割当では、必須参照順の後に実装を直接実行すること。「main agent は常に orchestrator である」と返答してはならず、委譲割当を新しいオーケストレーションサイクルへ変換してはならない。
- 役割スキルの禁止事項は、その役割として現在動作している実行者にのみ適用される。オーケストレーター禁止事項は親オーケストレーターを拘束し、本スキル配下で動作する委譲済み実装担当を拘束しない。
- `docs/task-governance/implementation-execution.md` の必須参照、ファイルカテゴリ再読義務、記録要件に従うこと。
- `docs/tasks/tasks.md` は必須のリポジトリ文脈として読む。ただし親オーケストレーターが delegated task の work-item path を既に渡している場合、その work item を選定済み active item として扱うこと。`docs/tasks/tasks.md` を使って別項目を再選定したり、新しいオーケストレーションサイクルを開始したりしてはならない。
- レビューフィードバックの是正スコープについては、`docs/task-governance/implementation-execution.md` の拘束力あるフルスコープレビュアー視点規則に従うこと。
- 実装規則のテキストをそのまま適用すること（`最小構成で済まそうとしてはならない。` を含む）。
- 現在の構成・アーキテクチャ（現行のコード構造そのもの）は固定の前提とする。新規実装・是正は現行の層境界の内側に収めることを基本とし、既存コードを優先的に流用する。現行コード構造を別構造へ作り替える大幅リファクタリング（モジュール/ドキュメント境界のゼロベース書き直しを含む）を実装義務として課してはならない。
- `adapters/` にコードを書く前に、実装がポートトレイト実装のみを公開していることを検証すること。`pub`、`pub(crate)`、`pub(super)` のいずれで宣言されていても、ポートトレイト実装でない項目はレイヤー違反であり、削除するかプライベートにしなければならない。stdin読み取り関数、プロンプト関数、JSONデコーダー、ターミナルI/O関数などのヘルパー関数はポートトレイト実装ではないため、以前 `pub(crate)` であったとしても、アダプターファイル内で `fn`（プライベート）でなければならない。
- `application/` にコードを書く前に、アダプターの具体型がインポートされておらず、`println!` / stdin読み取りが存在しないことを検証すること。
- `docs/architecture/hexagonal-implementation-rules.md` のレイヤーベース規則はファイル名固有規則より優先される。ファイル名固有の違反対象（例: `yubikey.md` の V1〜V16）が解消されているように見えても、レイヤーベースの違反が残っている場合、その項目は解消されていない。
- 変更を完了する前に、変更した各ディレクトリの層を `docs/architecture/hexagonal-implementation-rules.md` のディレクトリと層の対応規則で特定し、`docs/architecture/review-checklist.md` を開いて対応する層のすべてのチェック項目を適用すること。違反があれば、先へ進む前に解消すること。チェックリスト内容はここへ重複記載しない — 正本は `docs/architecture/review-checklist.md` である。
- コードを書く前に、変更予定の各ディレクトリの層を `docs/architecture/hexagonal-implementation-rules.md` のディレクトリと層の対応規則で特定し、`docs/architecture/review-checklist.md` にあるその層の「レビュー時の問い」（哲学的問い）を読み、予定する実装について各問いへ回答すること。いずれかの回答が「この実装は層の哲学に違反する」であれば、チェック項目が形式上通る場合でも実装を修正すること。哲学的問いに答えられない実装は未完了であり、提出してはならない。
- 層間で処理を移動する前に、`docs/architecture/hexagonal-implementation-rules.md` の規定済み責務境界（`domain rule` / `application orchestration` / `port contract` / `adapter translation` / `support technical primitive`）へ処理単位を分類し、その規定済み境界に置くこと。薄い port を保つために adapter/support へ業務判断を押し込んではならず、adapter を薄くするために support へ逃がしてはならない。support には backend 実装依存の技術補助、SDK 呼び出しの安全な補助、protection/zeroize/core dump 保護、業務判断を含まない変換を置けるが、業務判断、usecase 手順、固定 secret key の意味づけ、一意解決の業務規則、0件/複数件の domain failure 化、BWS check の外部検証 plan を置いてはならない。
