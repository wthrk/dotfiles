# タスク完了判定

この文書は、作業単位を完了扱いにできる条件を定義する。

## 完了判定の前提

- 必要レビュー役割は [implementation-review-judgement.md](implementation-review-judgement.md) に従う。
- 主成果物の差分が存在する。
- 必要レビュー役割の判定がそろい、集約後レビュー判定が `合格`。
- [implementation-execution.md の全経路閉鎖不変条件](implementation-execution.md#全経路閉鎖不変条件) の coverage、counterexample、自己照合、review 集約、および S2/S3/S4 で全値一致した [review-candidate identity](implementation-execution.md#比較-identity) の証跡が対象差分へ対応付けられている。
- ユーザー指定の GitHub issue / PR / 明示タスクの完了条件を満たす。
- 領域固有文書が追加条件を定める場合は、それを満たす。

## 実装作業の追加条件

- executable behavior を含む作業では、関連コードに実コード差分がある。
- 文書差分のみは、実装作業の完了根拠に使わない。
- 表面的な進捗指標だけを根拠に完了判定してはならない。

## 文書作業の扱い

- 文書主成果物の作業は、文書差分、handoff に統合した文書 flow・役割・参照経路・必要証跡・明示除外の coverage/counterexample/S1 自己照合、必要レビュー合格で完了判定できる。非適用の SDK、layer、device、lifecycle その他は理由と正本根拠を持つ明示除外に限り、適用項目の省略は受入れない。code 差分や実行テストを文書作業の代替証跡として要求しない。
- 完了済み作業台帳、confirmation、review artifact、current-cycle 記録の作成や同期は要求しない。

## 判定失効条件

次のいずれかが欠落する場合、完了判定を無効とする。

- 対象差分識別子。
- 集約後レビュー判定。
- 必須レビュー役割の判定。
- 必須化された確認結果。
- 全経路閉鎖不変条件の証跡、または穴のないことを示す集約根拠。
- 未解消の PR review thread への対応。

## コミット許可条件

- コミットは、対象差分、必須レビュー役割の結果、集約後レビュー判定 `合格` の記録を満たす場合に許可する。
- commit gate は、全経路閉鎖不変条件の証跡なし、穴を検出した差分、または局所 pass / test の一部通過だけを根拠とする差分を受入れてはならない。変更を「一括」等と呼ぶことも代替にならない。
- 補助記録の exact file-set、file-count、current-cycle 文言、対象パス列挙、台帳間同期は要求しない。
- 口頭・チャットのみの合格表明は許可根拠にならない。
