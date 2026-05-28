# global-documentation-remediation 確認記録（2026-05-26 現行サイクル）

この文書は、`docs/tasks/repo-governance/tasks.md` の作業項目 `ガバナンス文書整合` に対する 2026-05-26 現行サイクルの確認証跡である。AGENTS.md / AGENTS_ja.md の肥大化是正（役割判定の機構依存欠陥の修正と、領域固有詳細の正本移管）を対象とする。

## サイクル情報

- 区分: `current-cycle confirmation`
- 確認状態: `完了`
- 対象差分識別子: `working-tree-current-2026-05-26`
- 差分区分: `文書整合`

## 現行確認対象

- `AGENTS.md` / `AGENTS_ja.md`: Codex/exec の役割判定を「委譲された役割」基準へ修正した既存差分（working tree 既存）を温存し、追加で領域固有の secret-recovery 詳細を除去。
  - `## Critical Planning Gate` / `## 重要な計画ゲート` セクションを削除（計画手順・実装方針はオーケストレーター禁止事項節および `docs/secret-recovery/implementation-guidelines.md` が正本）。
  - `## Applying Document Instructions` / `## 文書指示の実行` 内の旧 13 箇条（Codex/exec の機構依存役割判定 1 箇条 + secret-recovery 領域固有の計画・進行・文書取り扱い規則 12 箇条）を 3 箇条へ集約。内訳は、機構依存判定を委譲役割基準へ正した役割判定箇条、secret-recovery 領域規則を `docs/secret-recovery/implementation-guidelines.md` へ委ねるポインター箇条、文書編集許可の適用範囲箇条。secret-recovery 領域規則の inline 詳細はこのポインター箇条へ集約して除去した。
- `docs/secret-recovery/implementation-guidelines.md`: AGENTS から除去した進捗記録規則のうち、正本に未収載だった 3 規則を既存の `### 確認` 節（実装単位ごとの secret-recovery 固有方針配下）の末尾へ追記して移管。新規見出しは作成していない。

## 規則の単一所有確認（除去規則 → 正本の所在）

- 文書是正依頼は直接修正（再解釈禁止）→ implementation-guidelines.md `## 実装方針`（既存）
- 主成果物別の code-first / doc-primary 扱い → implementation-guidelines.md `## 実装方針`（既存）
- 対象コードパスは開始点（呼び出し元/先・共有型・port/adapter・テストへ追従可）→ implementation-guidelines.md `## 実装方針`（既存）
- 文書のみ差分を実装進捗にしない → implementation-guidelines.md `## 実装方針`（既存）
- `文書整合`/`実装` 分離と `コード差分なし` 記録 → implementation-guidelines.md 既存 `### 確認` 節（末尾へ追記移管）
- コード差分なし時の `確認`/`レビュー` 停止・`実装状態` 据置き → implementation-guidelines.md 既存 `### 確認` 節（末尾へ追記移管）
- 前進遷移は同一変更セットの前提証跡を要する → implementation-guidelines.md 既存 `### 確認` 節（末尾へ追記移管）
- `コード差分なし` は前進根拠に使えない → implementation-guidelines.md `### 確認`（既存）
- code-primary では文書のみ作業を従属扱い → implementation-guidelines.md `## 実装方針`（既存）
- doc-primary は文書差分で前進可 → implementation-guidelines.md `## 実装方針`（既存）
- 整合に必要な文書編集は許可 / doc-primary は主成果物可 → implementation-guidelines.md `## 実装方針`（既存）
- 文書編集許可は委譲役割のみ / オーケストレーション専任中は直接編集禁止 → AGENTS `## Orchestrator Role — Absolute Prohibitions`（既存・重複につき除去）

## 確認手順と結果

- 確認手順: `git diff --check HEAD -- AGENTS.md AGENTS_ja.md docs/secret-recovery/implementation-guidelines.md`
- 確認結果: `空白エラーなし`
- 同期確認: `AGENTS.md と AGENTS_ja.md の変更箇所が節構成・規則内容ともに対応している（計画ゲート節削除・進捗規則ブロックのポインター化・Codex 節の役割記述）。`
- スコープ確認: `変更は AGENTS.md / AGENTS_ja.md / docs/secret-recovery/implementation-guidelines.md の 3 文書のみ。rust/ 配下は未変更。`

## 状態注記

- 役割分離注記: `本記録は実装担当による確認証跡であり、レビュー合格判定ではない。必須レビュー役割（変更種別に応じ、文書構成・AGENTS 変更を含むため参照整合レビュー担当を含む）の判定は別途委譲する。`
- 機構依存欠陥注記: `Codex/exec の役割判定欠陥は working tree に既存の修正差分として存在しており、本サイクルではこれを温存した上で領域固有詳細の除去を追加した。docs/ および .agents/skills/ の掃引では他に同欠陥の実例は検出されなかった。`
