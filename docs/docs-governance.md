# docs 文書運用規約

この文書は `docs/` 配下の文書運用規約を定義する。

## 配置規則

- `docs/README.md` は文書入口として扱い、ディレクトリ概要と `docs/` 配下項目の役割を記載する。
- `docs/architecture/README.md` は architecture 文書群の入口として扱い、ディレクトリ概要と配下項目の役割を記載する。
- `docs/secret-recovery/README.md` は secret-recovery 文書群の入口として扱い、ディレクトリ概要と配下項目の役割を記載する。
- `docs/secret-recovery/review-artifacts/README.md` は review-artifacts 文書群の入口として扱い、ディレクトリ概要と配下項目の役割を記載する。
- `docs/secret-recovery/review-artifacts/architecture-rules/README.md` は architecture-rules 文書群の入口として扱い、ディレクトリ概要と配下項目の役割を記載する。

## 記載規則

- 仕様・設計・実装規約の本文は対象ファイルに記載し、README には重複掲載しない。
- 作業進捗、現在の実装状況、レビューの実施ログは、進捗管理用ファイルに記載し、仕様・設計・規約ファイルには記載しない。
- 仕様・設計・実装規約などの恒久文書には、`現段階`、`後続 PR`、`実装後` など時点依存の進捗メモを記載しない。
- 恒久仕様文書は完成形の到達仕様を記載し、現行実装での利用可否や進捗は進捗管理用ファイルで確認する。
- secret-recovery の進捗入口は [secret-recovery/tasks.md](secret-recovery/tasks.md) に固定し、他文書に別の進捗入口を定義しない。
- secret-recovery の完了追跡は、tasks.md で固定実装単位ごとの成果物と完了条件を対応づけて管理する。
- secret-recovery の Phase/Issue レベルの完了判定は [secret-recovery/implementation-guidelines.md](secret-recovery/implementation-guidelines.md) の進捗状態判定規則を正本とし、`tasks.md` の作業項目完了をそのまま Phase/Issue 完了に読み替えない。

## タスク管理解釈規則

- プロジェクト内で `次のタスク`（`next task`）の指示を受けた場合、作業開始前に進捗管理用ファイルを参照し、未完了の先頭作業項目を現在の作業対象として扱う。
- secret-recovery では、先頭の未完了作業項目が executable behavior を含み `実装状態` が `未実装` または `実装中` の場合、`次のタスク` の主対象は実装・コード作業とし、固定実装単位トラッカーは従属情報として扱う。
- secret-recovery の進捗管理用ファイルは [secret-recovery/tasks.md](secret-recovery/tasks.md) を正本とし、進捗状態の更新が発生するたびに同ファイルを更新する。
- この正本性は固定実装単位と作業項目の進捗管理に限定し、Phase/Issue レベルの完了判定正本は上記判定規則に従う。
- README を含む他文書は進捗管理用ファイルへの導線だけを持ち、進捗状態そのものの正本を重複定義しない。

## 参照規則

参照は章番号ではなく、ファイルパスと見出し名で行う。文書更新時は参照元と参照先を同一変更で整合させる。

## 正本の扱い

同一内容の判断基準を複数ファイルで重複定義しない。正本を移す場合は旧ファイルの該当箇所を案内へ置き換えるか削除し、重複を残さない。
