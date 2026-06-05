# PR マージ可能化ループ

この文書は、PR を実際に merge する前の「マージ可能な状態」へ到達させるための共通運用を定義する。

## 対象

次のいずれかを含む依頼では、この文書を適用する。

- PR URL または PR 番号が指定されている。
- PR review 対応、AI review、Codex review、Copilot review が指定され、対象 PR を特定できる。
- 対象 PR が特定できる文脈で `@codex review` が指定されている。
- review thread の返信、修正、resolve が指定されている。
- checks、mergeability、merge 可能状態の確認が指定されている。

## ゴール

ゴールは、PR が merge 可能状態であると確認できることとする。少なくとも次を満たす。

- PR の mergeability が `mergeStateStatus: CLEAN` 相当である。
- 必要な checks が success である。
- 未解決の review thread がない。
- 最新 head に対する AI / Codex / Copilot review が no-issue である。
- 採用した指摘への修正、採用しない指摘への理由返信、対応済み thread の resolve が完了している。

実際の merge 実行は、この文書の責務に含めない。

## 反復手順

すべての操作は、現在アクターにすでに確立されている役割と、その役割に許可された権限の範囲内でのみ行う。現在役割で許可されていない実装修正、commit / push、PR 上の返信、review thread の resolve などは直接実行せず、適切な確立済み役割または PR 操作担当へ戻す事項として報告する。

1. PR 番号、base branch、head branch、head OID を確認する。
2. checks と mergeability を確認し、pending / failing / blocked の理由を特定する。
3. review、comment、review thread の inventory を作成し、未対応、対応済み、誤検出、権限不足で resolve 不能のものを分ける。
4. 最新 head に AI / Codex / Copilot review がない、または古い head への review しかない場合は、対象 PR が特定できる範囲で、必要に応じて `@codex review` などのリクエストを行う。PR が特定できない review 依頼は、この文書ではなく通常の差分レビューとして扱う。
5. 指摘ごとに採用または不採用を判断する。
6. 採用する指摘は、現在役割で許可されている場合に限り修正する。commit / push は、`workflow.md` のコミット着手ゲートなど既存 gate を満たし、現在役割で許可されている場合に限り行う。許可されていない場合、または gate を満たしていない場合は、修正対象として適切な実行主体へ戻す。
7. 不採用の指摘は、現在役割で許可されている場合に限り PR 上で理由を返信する。許可されていない場合は、返信が必要な事項として適切な PR 操作担当へ戻す。
8. 対応済み review thread は、現在役割で許可されており、かつ resolve 権限がある場合に限り resolve する。許可または権限が不足する場合は、resolve が必要な事項として報告する。
9. 新しい head OID に対して、checks、mergeability、未解決 thread、AI / Codex / Copilot review 結果を再確認する。
10. ゴールを満たすまで手順 2 から 9 を繰り返す。

## 指摘の扱い

- 指摘を採用する場合は、同じ欠陥クラスが変更セット内に残っていないか確認する。
- 指摘を不採用にする場合は、PR 上に判断理由を残す。
- 修正済み指摘は、返信または resolve によって PR 上の状態も閉じる。
- 誤検出と判断した指摘も、無言で放置せず理由を返信する。

## 完了条件

完了報告では、次を確認できる形で示す。

- 確認した PR と最新 head OID。
- checks の結果。
- mergeability の結果。
- 未解決 review thread がないこと。
- 最新 head に対する AI / Codex / Copilot review が no-issue であること。
- 採用修正、不採用返信、resolve の実施状況。

## 保留条件

次の場合は、merge 可能化を完了扱いにせず、保留理由を具体的に報告する。

- checks が pending または failing である。
- 外部 reviewer または bot review が未完了である。
- review thread の resolve 権限が不足している。
- merge conflict がある。
- branch protection、required approval、required check などの条件が未充足である。
- 最新 head に対する AI / Codex / Copilot review を取得できない。

保留報告では、確認した head OID、未充足条件、次に必要な外部操作または待機対象を明示する。
