# 秘密情報復旧基盤の実装ガイドライン

この文書は、secret-recovery で恒久的に参照する固定実装単位、役割分担、レビューサイクル、実装方針の正本である。

## 目的と対象範囲

目的は、秘密情報復旧基盤の実装を一貫した層構造、秘密値の所有境界、成果物境界に固定し、secret-recovery 作業全体で同一の判断基準を維持することである。対象は `dotfiles secrets` のコマンドフロー、関連するドメインモデル、ワイヤ形式、ポート、アダプター、補助機能、設計文書である。

## 参照正本

- 全体タスク運用: [../task-governance/workflow.md](../task-governance/workflow.md#タスク運用ワークフロー)
- 実装担当の強制義務: [../task-governance/implementation-execution.md](../task-governance/implementation-execution.md#実装担当の強制義務)
- タスクファイル契約: [../task-governance/task-file-contract.md](../task-governance/task-file-contract.md#タスクファイルに必須の項目最小)
- 状態遷移と完了判定: [../task-governance/progress-judgement.md](../task-governance/progress-judgement.md#進捗判定規則)、[../task-governance/task-completion-judgement.md](../task-governance/task-completion-judgement.md#タスク完了判定)
- 粗粒度進捗の扱い: [../task-governance/legacy-issue-tracking.md](../task-governance/legacy-issue-tracking.md#復元規則)
- secret-recovery 領域文書の参照入口（active work item が要求した場合）: [../tasks/secret-recovery/README.md](../tasks/secret-recovery/README.md)
- 一般構造の判断: [../architecture/hexagonal-implementation-rules.md](../architecture/hexagonal-implementation-rules.md#hexagonal-implementation-rules)
- 機能仕様と保存仕様: [secret-recovery-spec.md](secret-recovery-spec.md#新規マシン秘密情報復旧基盤)、[yubikey-secret-storage-design.md](yubikey-secret-storage-design.md#yubikey-秘密情報保存設計)

## 計画依頼の固定実装単位

secret-recovery の計画、実装、確認、レビューで扱う実装単位は次の 6 つに固定する。

1. `規約計画`
2. `実装計画`
3. `規約文書更新`
4. `確認`
5. `レビュー`
6. `必要時の後続対応`

## secret-recovery で使う役割

- `オーケストレーター`: 参照順を root active ledger [../tasks/tasks.md](../tasks/tasks.md#ルートタスク台帳) 起点に固定し、そこで確定した active work item が要求する実行統治参照（`docs/tasks/<area>/...`）へ追従させる。
- `実装担当`: [../task-governance/implementation-execution.md](../task-governance/implementation-execution.md#実装担当の強制義務) に従い、対象コードパス、修正範囲内既存コード、直接必要な隣接コードを再読し、実コード差分、確認証跡、重複した文書同期を避けるために必要最小限の文書整合差分を作る（構造是正や再設計の範囲を最小化する意味ではない）。
- `構造レビュー担当`: アーキテクチャ規約、作業定義文書、責務境界、依存方向、公開インターフェース境界に照らして判定する。
- `運用整合レビュー担当`: 状態遷移、証跡、台帳更新、履歴復元、参照整合に照らして判定する。
- `セキュリティレビュー担当`: 秘密値、認証情報、権限境界、永続化、ログ、外部入出力、危険な失敗時挙動に照らして判定する。
- `仕様適合レビュー担当`: 仕様書、設計書、作業定義文書、停止条件、成功条件、利用者向け挙動に照らして判定する。
- `参照整合レビュー担当`: 文書構成変更、タスク運用変更、スキル変更、`AGENTS.md` 変更がある場合に、パス、導線、正本参照、領域固定前提の混入を確認する。
- `進捗判定担当`: root active ledger である [../tasks/tasks.md](../tasks/tasks.md#ルートタスク台帳) と、active work item が要求する領域台帳 [../tasks/secret-recovery/tasks.md](../tasks/secret-recovery/tasks.md#新規マシン秘密情報復旧基盤タスク)、粗粒度進捗 [../tasks/secret-recovery/issue-11-progress.md](../tasks/secret-recovery/issue-11-progress.md#11-系粗粒度進捗) の状態更新を行う。
- `履歴復元担当`: `#11` 系の粗粒度進捗や失われた milestone を git 履歴から復元する。
- `タスク定義整備担当`: [../tasks/secret-recovery/tasks.md](../tasks/secret-recovery/tasks.md#新規マシン秘密情報復旧基盤タスク) と [../tasks/secret-recovery/work-items/README.md](../tasks/secret-recovery/work-items/README.md#secret-recovery-work-items) の責務分離が崩れた場合に文書分割と参照張替えを行う。

## planning / implementation / review の役割分担

- `規約計画` と `実装計画` は実装担当が行う。secret-recovery では対象コードパスと直接必要な隣接コードパスを読まずに一般論だけで計画してはならない。
- `規約文書更新` と `確認` は実装担当が行う。文書更新だけで主作業を消化してはならない。
- `レビュー` は複数レビュー担当で行う。secret-recovery では最低でも `構造レビュー担当`、`運用整合レビュー担当`、`セキュリティレビュー担当`、`仕様適合レビュー担当` を起動し、文書構成変更、タスク運用変更、スキル変更、`AGENTS.md` 変更がある場合は `参照整合レビュー担当` も起動する。
- `進捗判定` は進捗判定担当が行う。オーケストレーターおよび current executor は、フォールバック宣言の有無に関係なく `進捗判定担当` の代替実行者になってはならず、`確認` / `レビュー` / `実装状態` の前進記録を直接記入してはならない。証跡が欠ける前進更新は無効とする。
- `#11` 系の粗粒度進捗が欠落している場合は、実装開始前または並行で履歴復元担当を入れる。

## レビューサイクル

1. 実装担当が主成果物を更新する。
2. 実装担当が `確認` を行い、追試可能な証跡を `docs/tasks/secret-recovery/review-artifacts/` に残す。
3. 複数レビュー担当が役割別に差分と証跡を確認し、個別判定を返す。
4. 差戻しがある場合は同一作業項目内で再実施する。
5. 進捗判定担当は、複数レビュー担当の結果と他の必要証跡が同一変更セットに揃った時だけ root active ledger [../tasks/tasks.md](../tasks/tasks.md#ルートタスク台帳) と、active work item が要求する [../tasks/secret-recovery/tasks.md](../tasks/secret-recovery/tasks.md#新規マシン秘密情報復旧基盤タスク) / [../tasks/secret-recovery/issue-11-progress.md](../tasks/secret-recovery/issue-11-progress.md#11-系粗粒度進捗) を前進させる。
6. コミット関連作業は、上記 1-5 の結果が台帳・レビュー成果物・必要な進捗記録へ反映された後にのみ着手できる。チャット上の完了宣言だけで着手してはならない。

## 実装方針

- 実装担当はアーキテクチャ規約と領域固有規約へ厳密に適合させなければならない。
- 最小構成で済まそうとしてはならない。
- 最小差分化や継承された既存構造の温存は目的ではなく、それらが既存のアーキテクチャ、仕様、作業定義への厳密適合を阻害する場合は適合構造への再設計を優先し、当該適合が満たされるまで修正対象から外してはならない。
- 作業定義文書の `完了の判定条件` に列挙された違反が 1 件でも残っている場合、実装担当は「ブロッカーなし」「完了」「動作確認済み」を報告してはならない。部分完了は `残留違反リスト` を明示した上で「部分進捗」として報告し、全件解消後に完了報告を行う。
- 「動作する」という事実は完了の根拠にならない。作業定義文書の `完了の判定条件` / `構造完了条件` / `レビュー合格条件` の全件充足が完了の唯一の根拠である。
- 全面再設計（作業種別: `モジュール構造のゼロベース書き換えを含む規約適合リファクタリング`）の作業項目では、作業定義文書の `規約違反の解消対象` に列挙された違反を全件解消することが完了条件である。部分的な再編のみで完了報告してはならない。
- [../tasks/secret-recovery/tasks.md](../tasks/secret-recovery/tasks.md#新規マシン秘密情報復旧基盤タスク) の `対象コードパス` は実装開始点であり、直接必要な呼び出し元、呼び出し先、共有型、port / adapter、対応テストへは追ってよい。
- 修正範囲内既存コードと直接必要な隣接コードに見えている規約違反は、今回の再編対象から外してはならない。
- executable behavior を含む作業では、主成果物は実コード差分である。文書差分だけで実装前進やレビュー準備完了を主張してはならない。
- [../tasks/secret-recovery/tasks.md](../tasks/secret-recovery/tasks.md#新規マシン秘密情報復旧基盤タスク) で主成果物が `文書差分` と宣言された作業項目は、必要な確認・レビュー証跡を満たす限り、文書差分を主成果物として前進してよい。
- [../tasks/tasks.md](../tasks/tasks.md#ルートタスク台帳) は active work item 選定と repository-wide 進捗更新の正本であり、[../tasks/secret-recovery/tasks.md](../tasks/secret-recovery/tasks.md#新規マシン秘密情報復旧基盤タスク) は active work item が要求する領域台帳/履歴として扱う。仕事の内容定義は [../tasks/secret-recovery/work-items/README.md](../tasks/secret-recovery/work-items/README.md#secret-recovery-work-items) を正本とする。
- `#11` 系の粗粒度進捗は [../tasks/secret-recovery/issue-11-progress.md](../tasks/secret-recovery/issue-11-progress.md#11-系粗粒度進捗) を正本とし、進捗台帳に混在させない。
- 利用者が文書修正を明示した場合は、その依頼を指定文書への直接修正として優先して扱う。加えて、台帳で主成果物が `文書差分` と宣言された作業項目では、利用者の別途明示依頼がなくても文書差分を主成果物として扱ってよい。

## 実装単位ごとの secret-recovery 固有方針

### `規約計画`

- 適用する secret-recovery 仕様、設計、hexagonal 規約の範囲を確定する。
- 対象作業項目と `#11` 系粗粒度進捗の対応を確認する。

### `実装計画`

- 対象コードパスと直接必要な隣接コードパスを読んだ観察結果に基づいて、再編対象と更新順序を確定する。
- [../tasks/secret-recovery/work-items/README.md](../tasks/secret-recovery/work-items/README.md#secret-recovery-work-items) の完了条件と、現行コードの違反箇所を対応づける。

### `規約文書更新`

- 実装差分に従属して、重複した文書同期を避けるために必要最小限だけ更新する（構造是正や再設計の範囲を最小化する意味ではない）。
- 作業定義の不足が見つかった場合は [../tasks/secret-recovery/work-items/README.md](../tasks/secret-recovery/work-items/README.md#secret-recovery-work-items) から対象文書を更新し、[../tasks/secret-recovery/tasks.md](../tasks/secret-recovery/tasks.md#新規マシン秘密情報復旧基盤タスク) は参照だけを維持する。

### `確認`

- 変更後の対象差分に対して必要な確認だけを行い、`docs/tasks/secret-recovery/review-artifacts/` へ記録する。
- `コード差分なし` の暫定記録は前進根拠に使えない。

### `レビュー`

- 作業定義文書の `レビュー合格条件` とアーキテクチャ規約に対する厳密適合で判定する。
- 不合格時は差戻し対象を同一作業項目へ戻す。

### `必要時の後続対応`

- レビューで必要と判定された追従更新と、コミット関連作業の委譲記録だけを扱う。
- コミット関連作業の委譲は、`レビュー` 集約判定と `進捗判定` 記録が正本成果物に反映済みである場合に限る。未反映の場合は委譲を開始してはならない。
