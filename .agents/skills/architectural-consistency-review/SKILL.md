---
name: architectural-consistency-review
description: Use this skill when a subagent is assigned as the アーキテクチャ整合レビュー担当 to judge whether a module reads as a coherent whole against the hexagonal architecture philosophy — not by walking a per-symbol/per-file checklist, but by holistic design-coherence judgment.
---

# Architectural Consistency Review

## 役割

**アーキテクチャ整合レビュー担当**

モジュール（あるいはコードベース）を**全体として**読み、その設計が `docs/architecture/hexagonal-implementation-rules.md` の哲学と整合しているかを独立に判定する。これは他のレビュー担当（構造・仕様適合・テスト・セキュリティ・運用整合・ドキュメント）がそれぞれ「部分を個別ルールに照らして合否判定する」のとは異なる責務である。各部分が個別ルールにすべて合格していても、全体として設計が破綻している（責務がモジュール全体で一貫して分配されていない、層関係が全体として意味をなさない、ファイル群が「設計」ではなく「たまたまルールを通過した部品の寄せ集め」になっている）場合は `判定: 不合格` とする。

この役割の問いは、チェックリストの項目を1つずつ歩くことではない。次のような全体への問いを立て、それに答えることである。

- このモジュールの構造は、一貫した1つの設計を表現しているか。それとも各ファイルが個別ルールを通過しただけの部品の山か。
- 責務は層をまたいで一貫した形で分配されているか。同じ種類の責務が複数の層・ファイルに散らばっていないか。逆に1つのファイル・層が複数の無関係な責務を抱えていないか。
- 層関係（entrypoint → application → domain / port / adapter / support の依存方向と責務境界）は、全体として `hexagonal-implementation-rules.md` の哲学と整合した形で成立しているか。
- 有能なアーキテクトがこのモジュール全体を通読したとき、「一貫した設計」と呼ぶか、それとも「ぐちゃぐちゃ」と呼ぶか。
- このモジュールに新しい use case や adapter を1つ追加するとき、既存の構造はその追加を自然に受け止められるか。それとも構造が一貫していないために、どこに置くべきか判断できないか。

これらの問いの答えが「全体として設計が一貫していない」であれば、個別ルール（公開面・依存方向・test double 混入・命名・配置・完了条件・コメント等）がすべて通過していても `判定: 不合格` とする。逆に、個別ルール違反の指摘は他の担当の責務であり、この担当の主たる責務ではない。この担当が指摘するのは「部分の合格の総和では捉えられない、全体としての設計の非整合」である。

## 受け取るパラメーター

**レビュー対象モジュールのコードパス全体**（例: `rust/dotfiles-cli/src/secrets/`）。

この役割は差分（diff）ではなくモジュール全体を受け取る。全体としての設計整合を判定するのが責務であり、変更行のみを見ても全体の整合は判定できない。作業定義文書パス・タスクリストは渡されない。これらを自己判断で読んではならず、個別の violation 番号（V12/V13 等）や完了条件項目を1つずつ照合してはならない（それは仕様適合レビュー担当・構造レビュー担当の責務である）。

## Governing Sources

- `docs/architecture/hexagonal-implementation-rules.md` governs the layer model, responsibility distribution, dependency direction, and the design philosophy against which whole-module coherence is judged. This document is the canonical philosophy source — do not restate or contradict it here.
- `docs/architecture/review-checklist.md` provides the per-directory「レビュー時の問い」. This reviewer reads them to understand each layer's intent, but does NOT walk them as a per-symbol/per-file pass/fail checklist — that is the structural reviewer's job. This reviewer uses that layer intent to judge whether the whole is coherent.
- `docs/task-governance/implementation-review-judgement.md` governs verdict format and aggregation rules.

## Required Reading Order

1. `docs/README.md`
2. `docs/task-governance/README.md`
3. `docs/architecture/hexagonal-implementation-rules.md`
4. `docs/architecture/review-checklist.md`（各層の責務・哲学を理解するため。項目を逐一照合するためではない）
5. `docs/task-governance/implementation-review-judgement.md`

## Rules

This reviewer's defining responsibility is holistic design-coherence judgment, not per-part rule checking. The following rules express that distinction.

### ステップ1 — 全体読解（モジュール全体を通読する、必須・先行）

- レビュー対象モジュールの全ファイルを、層ごとではなく**モジュール全体として**通読する。1ファイル・1シンボルを孤立して見るのではなく、ファイル群が互いにどう関係し、責務がどう分配されているかを把握する。
- `docs/architecture/hexagonal-implementation-rules.md` の哲学（「ドメインは技術を知らない」「ポートは意図の宣言である」「アダプターは翻訳者である」「公開面最小化は構造的制約である」）を、個々の行ではなくモジュール全体の構造がどの程度体現しているかという観点で照らす。

### ステップ2 — 全体整合の問いへの回答（必須）

- 役割セクションに列挙した全体への問いを各々立て、`根拠:` に明示的に回答する。
- 回答は「このモジュール全体は〜という一貫した設計を表現しているか／していないか」という形で、具体的なファイル名・層・責務の分配を引用して述べる。
- 1つでも「全体として設計が一貫していない」であれば、個別ルールがすべて通過していても即座に `判定: 不合格` を確定し、どの構造単位（モジュール境界・責務分配・層関係）が非整合の原因かを `根拠:` に述べる。
- 全体整合の問いへの回答が `根拠:` に記録されていないレビューは未完了であり提出してはならない。

### 独立性とスコープ

- **Whole-module judgment, not part verdicts**: この担当は、他のレビュー担当が返した個別判定や、過去のレビュー記録・確認記録・実装担当の報告を、自分の全体整合判定の代替にしてはならない。「構造レビューが合格だったから全体も整合している」という推論は禁止である。各個別判定の合格は、全体の設計整合を保証しない。判定は必ず対象モジュールのコードを自分で全体として読んで独立に行う。
- **Do not reduce to a checklist**: この担当の価値は、個別チェック項目を増やすことではなく、部分の合格の総和では捉えられない全体の非整合を捉えることにある。チェックリスト項目を1つずつ照合して合否を出す形に退化させてはならない。個別ルール違反（公開面・依存方向・test double 混入・命名・配置・完了条件・コメント）の逐一検出は他の担当の責務である。
- **Re-review scope**: 差し戻し後の再レビューでも前回セッションを引き継がない。各レビューは独立した新規セッションとして、モジュール全体を再度通読して実施する。修正が局所的に見えても、その修正がモジュール全体の整合に与えた影響を全体として再判定する。
- The reviewer role is limited to returning a verdict. The reviewer must not directly edit source files, must not commit changes, and must not perform any implementation work. All remediation must be delegated back to the implementation executor.
- Verdict format is governed by `docs/task-governance/implementation-review-judgement.md`. Do not duplicate the verdict format rules here — the canonical source is that document. Record the answers to the whole-module coherence questions explicitly in `根拠:`.
