---
name: implementation-execution
description: サブエージェントが実装作業を割り当てられ、リポジトリの実行義務に従って diff を作成するときに使う。
---

# Implementation Execution

## Actor Binding

このスキル有効時の現在アクターは **implementation executor**。

## Governing Sources

- `docs/task-governance/implementation-execution.md`
- `docs/task-governance/security-obligations.md`
- `docs/docs-governance.md`
- `docs/architecture/hexagonal-implementation-rules.md`
- `docs/architecture/review-checklist.md`

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/task-governance/implementation-execution.md`
4. `docs/task-governance/security-obligations.md`
5. `docs/docs-governance.md`
6. `docs/architecture/hexagonal-implementation-rules.md`
7. `docs/architecture/review-checklist.md`
8. 委譲された GitHub issue、PR、明示タスク、または handoff
9. 委譲領域の正本仕様・基本設計・runbook。SDK 調査前に目的、storage target、各値の generate/save/read/use/dispose、状態遷移、利用者入力/出力、禁止事項を再構成する
10. 手順 9 の後に限り、その product flow を実現する vendor / SDK の全体フロー文書、仕様、公式サンプル、API documentation、versioned source
11. コード構造が対象の場合は architecture 文書

## Rules

- delegated task の orchestration は完了済みとして扱う。
- 同じ delegated task について `/orchestration` を起動せず、`$dotfiles-task-governance` を orchestration、役割変更、作業単位の再選定に使わない。
- 作業単位を再選定せず、同じ実装割当に対して subagent を起動しない。
- 編集前に対象ファイル、直接依存、呼び出し元/呼び出し先、テスト、handoff finding を読む。
- 再構成した product flow の各遷移（generate、save、read、use、dispose、input、output、failure、cleanup）を code、test、doc comment と照合する。SDK 資料は実装手段の根拠であり product design を置換しない。両者が矛盾するか product の遷移が未定義なら、推測せず設計判断を要求する。
- secret-recovery では、正本の利用者契約「利用者は YubiKey を挿して復旧コマンドを実行するだけ」を強制する。復旧と `verify-yubikey --all` は YubiKey 保存の BWS credentials を内部で使ってよいが、master password、session、PIV PIN、secret の environment/argv、YubiKey OTP、その他の対話 input を要求してはならず、これらを stdout、stderr、log、一時 file、永続 environment へ出してはならない。
- 割り当て差分を作成し、選択した検証を実行し、対象差分・コマンド・結果・未実施確認・残リスクを報告する。
- 詳細な実装義務は `docs/task-governance/implementation-execution.md` が所有する。
