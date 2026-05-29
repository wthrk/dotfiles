# docs 文書運用規約

この文書は `docs/` 配下の配置、正本、重複禁止を定義する。

## 配置規則

- `docs/README.md`: 文書全体の入口
- `docs/task-governance/README.md`: 共通運用規約の入口
- `docs/tasks/README.md`: 領域別台帳の入口
- `docs/<area>/README.md`: 領域仕様の入口

## 正本規則

- 共通運用規約は `docs/task-governance/` に置く。
- active work item の選定正本は `docs/tasks/tasks.md` とする。
- 領域別の作業管理は `docs/tasks/<area>/` に置く。
- `docs/tasks/<area>/tasks.md` は存在する場合に限り、領域内の補助台帳/履歴として扱う。
- レビュー結果の正本は `review-artifacts` とする。
- 台帳はレビュー証跡への参照を保持すればよく、同一事実の重複記録を必須化しない。

## 記載規則

- README は導線のみを記載し、本文規約を再掲しない。
- 仕様・設計・運用規約・証跡の責務を混在させない。
- 文書是正では、無関係文書への同期更新を必須にしない。
- 文書規約は、後付けの文書書換えだけで充足できる形式要件を gate にしてはならない。実行、レビュー品質、検証正確性のいずれも実質的に改善しない exact file-set/file-count 台帳、重複 scope 台帳、actor/run bookkeeping、current-cycle 文言の完全一致、confirmation/review artifact の exact 同期などは必須化しない。
- review artifact / confirmation / 台帳は補助記録であり、実装・レビュー・commit gate・PR review 対応の代替ではない。修正済み PR コメントへの返信/resolve と誤検出コメントへの説明返信は維持するが、補助記録の同期そのものを目的化しない。

## 参照規則

- `docs/tasks/tasks.md` で active work item を選定し、以降はその項目が要求する参照先を必須参照として扱う。
- `docs/tasks/<area>/tasks.md` は active work item が要求している場合のみ必須参照とし、未要求の場合は存在を前提にしない。
- `docs/tasks/<area>/tasks.md` に active work item 選定子（`現在の作業項目`）を置いてはならない。
- 参照はファイルパスと見出し名で行う。
- 正本を移す場合は旧記述を削除または参照化し、二重正本を残さない。
