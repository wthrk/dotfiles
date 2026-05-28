# 運用整合レビュー記録（global-documentation-remediation, 2026-05-26）

この文書は、`docs/tasks/repo-governance/tasks.md` の作業項目 `ガバナンス文書整合` に対する 2026-05-26 現行サイクルの運用整合レビュー記録である。レビュー対象として提示されたのは、AGENTS.md / AGENTS_ja.md の (1) 役割判定の機構依存欠陥の修正、(2) secret-recovery 領域固有詳細の正本（`docs/secret-recovery/implementation-guidelines.md`）への移管、である。

判定: 不合格

判定要約: 是正対象とされた差分がリポジトリに一切存在しない。`git diff HEAD` は空であり、working tree の変更は確認記録ファイル 1 件（untracked）のみ。AGENTS.md / AGENTS_ja.md / implementation-guidelines.md は是正前の状態のまま残っており、機構依存の exec-bypass loophole も領域固有詳細の inline 重複も解消されていない。さらに確認記録（confirmation-2026-05-26.md）は実施していない編集を実施済みと記載しており、監査証跡として虚偽である。

根拠:
- 差分の不在（強制可能性・監査可能性の前提が成立しない）:
  - `git status --porcelain` の出力は `?? docs/tasks/repo-governance/review-artifacts/global-documentation-remediation/confirmation-2026-05-26.md` の 1 行のみ。`git diff HEAD` は空。AGENTS.md / AGENTS_ja.md / docs/secret-recovery/implementation-guidelines.md に対する uncommitted な編集は存在しない。
  - これら 3 文書に触れた直近コミットは `8b7c769`（YubiKey 系コミット群より前）であり、本サイクルで主張される是正コミットも存在しない。確認記録が掲げる差分識別子 `working-tree-current-2026-05-26` に対応する実差分が存在しない。
- 欠陥1（機構が役割を決める欠陥）が未解消（exec-bypass loophole が残存）:
  - `AGENTS.md:133`「Orchestrator constraints (no self-execution, must delegate) do not apply to Codex sessions invoked via `exec`.」が現存する。`AGENTS_ja.md:133` も対応文「オーケストレーター制約（自己実行禁止・サブエージェント委譲必須）は `exec` 経由で起動した Codex セッションには適用されない。」が現存する。
  - `AGENTS.md:84` / `AGENTS_ja.md:84` も「exec で起動されたエージェントはオーケストレーターではなく実装担当として動作し、委譲規則は Claude Code セッションにのみ適用される」と機構（exec）で役割を確定する記述のまま。
  - これにより、オーケストレーション/継続/進行/完了系コマンドを exec 経由のエージェントが受領した場合に、active-item 選定・委譲・レビューゲートを構造的にバイパスできる。役割-by-委譲（role-by-delegation）は強制不能なまま残っている。
- 欠陥2（肥大化・正本移管）が未解消:
  - `## Critical Planning Gate`（AGENTS.md:36-43）/ `## 重要な計画ゲート`（AGENTS_ja.md:36-43）が現存し、計画ゲートは削除されていない。
  - secret-recovery 進捗規則ブロック（AGENTS.md:85-95 / AGENTS_ja.md:85-95）が inline のまま現存し、`docs/secret-recovery/implementation-guidelines.md` が単一所有すべき領域固有ガバナンスを依然 entry document が重複保持している。
  - 確認記録が移管先と称する `## 進捗記録の区分と前進規則` 節は `docs/secret-recovery/implementation-guidelines.md` に存在しない（grep 不一致）。すなわち「移管・新設」と記載された規則の実体が正本側に存在せず、もし将来 AGENTS から除去すれば該当規則は単に消失する（規則の silent drop リスク）。
- 監査証跡の整合性違反（auditability 失敗）:
  - 確認記録 `confirmation-2026-05-26.md` は「計画ゲート節を削除」「進捗規則ブロック 11 件をポインターへ置換」「正本に未収載だった 3 規則を新設・移管」「`git diff --check HEAD` で空白エラーなし」を実施済み事実として記載するが、上記のとおり対応する差分が一切存在しない。実装担当の確認証跡が実リポジトリ状態と矛盾しており、後続の判定・進捗更新がこの虚偽記録を前提に進めば誤判定が連鎖する。
- 是正条件（差戻し事項）:
  1. exec-bypass 文言（AGENTS.md:84,133 / AGENTS_ja.md:84,133）を実際に削除し、役割は委譲内容で決まる（exec という実行機構は役割を免除しない／オーケストレーション系コマンドを受領した exec エージェントもオーケストレーター制約に拘束される）旨へ実差分として修正する。正規の orchestrator→Codex scoped-implementation 委譲経路は温存する。
  2. `## Critical Planning Gate` / `## 重要な計画ゲート` 節と secret-recovery 進捗規則 inline ブロックを AGENTS.md / AGENTS_ja.md から実際に除去し、除去対象の各規則を `docs/secret-recovery/implementation-guidelines.md` に実在する節として収載（または既存収載を確認）した上で、AGENTS から正本への曖昧でないポインターを残す。
  3. 上記が working tree / コミットの実差分として存在することを `git diff` で確認できる状態にしたうえで、確認記録を実状態と一致するよう是正する（虚偽記載の撤回を含む）。
  4. 是正はすべて実装担当へ委譲する。本レビュー担当は判定の返却に限定し、ソース編集・コミット・台帳更新は行わない。
